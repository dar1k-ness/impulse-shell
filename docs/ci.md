# Checks: the gate, locally and in CI

One script decides whether a change is good: `tools/testenv/ri-gate`, run in the `ri-testenv`
image (`tools/testenv/Containerfile`), which holds the pinned toolchain and every reference
tool the tests compare against. The same script runs locally and in the CI workflows.

## Before a merge: one command

```sh
tools/ci/gate --workspace
```

GitHub Actions runs only the shader precision tests on a pull request, and only on one that
touches the GPU code or the image (see [CI](#ci)), so this is the check of every pull request: run it on the branch with `main` merged in, and put the summary it prints into
the pull request. Exit status 0 means every step passed.

It needs Linux, git and Docker (usable by your user), nothing else: no Rust, no libraries.

| | Measured on 8 cores, 32 GB RAM |
| --- | --- |
| First run: builds the image | 9 min |
| Full gate after a merge of `main` that rebuilds most crates | about 20 min |
| Full gate, nothing to rebuild | 5 min |
| Disk: image | 8.1 GB |
| Disk: `~/.cache/ri-gate` (target dir, Cargo registry) | 24 GB |

What it does:

1. Builds the image if this checkout's one is missing. Its tag is a hash of
   `rust-toolchain.toml` and `tools/testenv` (`tools/ci/image-tag`), so a change to either
   builds a new image and nothing else does.
2. Starts a container with the checkout at `/src` and the cache directory as the target dir
   and Cargo registry. The flags are the ones the tests need (user, mount and PID namespaces):
   `tools/ci/in-testenv`.
3. Gives a new modification time to every file of the checkout that differs from the tree
   the gate ran on last (`tools/ci/tree-times`, see below).
4. Runs `ri-gate` there. Cargo runs as the user `tester`, who has your uid and gid in the
   container: what a run writes in the checkout and the cache is yours at every moment, also
   when the container is killed. (As root, the checkout and the cache are handed to `tester`
   for the run and back when it ends.)

`tools/ci/gate --help` prints the arguments and the steps. An argument or a step the gate
does not know is an error: nothing is built or run.

### Two trees, one target dir

Every checkout is mounted at `/src` and built into the same target dir, and cargo decides by
modification times alone whether what it built from a file is still good (Cargo Book:
[`rerun-if-changed`](https://doc.rust-lang.org/cargo/reference/build-scripts.html#rerun-if-changed),
[`checksum-freshness`](https://doc.rust-lang.org/cargo/reference/unstable.html#checksum-freshness)).
A tree whose files are older than the last build (a second `git worktree`, an unpacked
archive) was taken for the tree built before: the tests ran the other tree's code, or a
build script kept a list of files that are no longer there.

`tools/ci/tree-times` keeps a record of the tree in the target dir (`ri-gate-tree`: each
file and directory with a hash of its content) and, before every command the gate runs, sets
the modification time of what differs from the record to now. Cargo then builds again what
reads those files, and nothing else. `crates/shell/tests/gate_tree_times.rs` holds it: a tree
put at the same path with older times is built as a clean target dir builds it.

- What it does not record: directories named `.git` and `target`, at any depth. It does not
  read `.gitignore`: a file git ignores is recorded like any other, so a build script that
  reads one is covered. `target` is where cargo writes, and no build script here reads from a
  `target` or from `.git`: what they name with `rerun-if-changed` is the `qml` tree,
  `crates/qml/qmltypes`, `crates/shell/tests/scenes`, `crates/colorgen/mcu`,
  `crates/rigen/check/cases`, `crates/rigen/src` and the workspace `Cargo.toml`. One that
  starts to read from there must not rely on the gate seeing a change.
- One run at a time uses a cache directory (`$RI_GATE_DIR/lock`, taken by `tools/ci/gate` and
  by `tools/ci/in-testenv` when it is started on its own), and `tree-times` itself refuses a
  second run on the same record: two trees are never recorded at once.
- The record is written last, in one rename. A run killed before that leaves the old record;
  the next run finds the same differences and gives them a new time again.
- One checkout built into several cache directories (`RI_GATE_DIR`): each has its own record
  and compares the checkout with the tree it built last, so each builds again what changed
  since its own last run. A time given by a run for one directory is later than the builds
  of the other, which at worst builds again there what did not need it; nothing is skipped.

| | Cost |
| --- | --- |
| Disk | The record, 0.7 MB. No second target dir. |
| Each run | 0.8 s to hash the checkout (4800 files, 42 MB). |
| First run with a target dir that has no record | Every file is new to cargo: the workspace's own crates are built again, the dependencies are not. |
| The checkout | Files that differ from the tree of the last run get a new modification time; their content is not touched. |

The two other ways cost more. A target dir for each tree is 24 GB and a full build for each
(a 124 GB disk has been filled by one). Cargo's `checksum-freshness` needs a nightly
toolchain and leaves the files a build script reads to modification times.

The steps, in order. A failed step does not stop the next ones; the summary at the end lists
each step's result and time. Before it the gate prints every test no step ran, with the
reason its `#[ignore]` gives (`tools/ci/skipped-tests`, which also runs on its own).

| Step | What it runs |
| --- | --- |
| `tests` | `cargo nextest run --features shell/interp`: unit and integration tests, with every comparison against the reference tools; `RI_REQUIRE_GL=1`, so a GL test fails rather than passes unrun without a context. |
| `no-interp` | `qml` built without the interpreter: its `no_interp` test. |
| `rigen` | The `crates/rigen/check` workspace (interpreter against compiled JS); with `--workspace` also every `qml` script, failing if one does not compile. |
| `aot` | `riaotcheck --strict --v4` on `qml`: compiled against interpreted, sampled bindings replayed in Qt's QV4. Only with `--workspace`. |
| `fmt` | `cargo fmt --check`, both workspaces. |
| `clippy` | Clippy with `-D warnings` on every target, interpreter and `tools` on, and on the `crates/rigen/check` workspace. |
| `shipped-build` | The shell without the interpreter (`-p shell`, `shipped` profile; `RI_COMMIT=gate`, since that build does not compile without a commit to name and is installed nowhere), then `tools/install.sh` itself: `tools/build.sh` builds the release profile, names the checkout's commit and fails if the binary names a directory of the machine; installed into `/target/shipped-install`. |
| `shipped-tests` | Every shell test without the interpreter (`startup_errors`, `shipped_binary`, `install_rollback` among them); then the installed binary: `--version` says it has no interpreter, it runs the tree it was installed with, finds it when started as `impulse-shell -c impulse-shell`, holds no interpreter source paths, draws its bar from that tree, started that way (`shipped_first_frame`), and opens its panels there under a sway of its own: both sidebars, overview, session screen, lock, a notification, the vertical bar and the settings window each come on screen with a picture, react to a click and to a key (the notification and the bar take no keyboard), and log no script error (`shipped_panels`). |
| `shipped-clippy` | Clippy on that build. |
| `deny` | `cargo deny`: advisories, bans, licenses, sources, both lock files. |
| `gpu-precision` | The shader precision tests on llvmpipe, failing (not skipping) without a GL context. |

Without `--workspace` the gate covers the packages `qml` and `shell` only.

Builds use the `gate` profile of `Cargo.toml`: release without LTO, with debug assertions
on, so `debug_assert!` runs in the tests. The `shipped-*` steps use the `shipped` profile,
the same without debug assertions, as in the installed shell.

The pixels of a scene are compared with Qt Quick through OpenGL on llvmpipe (`crates/shell/tests/support/qt_gl.rs`); the CPU raster's linear filter (`layer::linear` in `qsg`) repeats its arithmetic, a weight of 0..=255 and the half rounded to even after each of the three blends, and so does its write of a pixel (`raster::blend_f`: the colour becomes bytes, the half rounded to even, then the bytes are blended with `mul255::nearest`). Its triangles (`Canvas::tri_with` in `raster`) repeat llvmpipe's as well, so that an antialiased edge pixel whose colour falls half way between two bytes rounds the same way: the viewport transform (`raster::viewport`) and a pixel's colour are fused multiply-adds, a vertex colour is its byte times the float nearest 1/255 (`layer::unorm`), and the GPU path gives llvmpipe the `gl_Position` Qt's shader computes, not pixels to turn back into it. The fused multiply-add is `raster::fma`, computed in software, so our picture does not depend on the processor. That llvmpipe fuses is read from Mesa's source (`lp_bld_arit.c` `lp_build_fmuladd`, `llvm.fmuladd`; `lp_bld_misc.cpp` turns `+fma` on by the processor), on a processor that has the instruction; what Qt's own picture is on a processor without it has not been run. On a real video card the driver rasterizes, filters and blends.

### Part of the gate

```sh
tools/ci/gate --workspace --only fmt,clippy        # these steps
tools/ci/gate --workspace --skip tests             # all but these
tools/ci/gate --only tests --test keyboard_focus   # with --only tests, what follows goes to nextest
tools/ci/gate --help                               # the usage; it and an unknown argument run nothing
tools/ci/gate --run as-tester cargo nextest run --cargo-profile gate -p qml ports::   # any command
tools/ci/gate --run bash                           # a shell in the image
```

`RI_GATE_DIR=<dir>` moves the cache. One run at a time uses a cache directory; a second one
stops with a message.

### The properties of Qt types against Qt 6.12: `qt_props`

```sh
tools/ci/gate --only tests --test qt_props --no-capture
```

It reads every property of the 49 Qt types `qml/` uses (3369 rows: with nothing written,
after an assignment, after `= undefined`, from a binding that gives a value, `undefined` and
a value again) and compares each row with what Qt 6.12 gave. It prints
`rows 3369, as in Qt N, differ M` and fails for a differing row that is not in
`crates/shell/tests/qt_props/differ/<Type>.txt`, and for a row listed there that is now
Qt's: a fix takes its rows out of the list. What the engine gave is left in
`<cache>/target/tmp/qt_props/<Type>.txt` (`~/.cache/ri-gate` unless `RI_GATE_DIR` is set), to
compare with `crates/shell/tests/qt_props/qt/<Type>.txt`. The header of
`crates/shell/tests/qt_props.rs` says what a row is and how the Qt side was written.

### Not in the gate: a power cut during an install

```sh
sudo tools/testenv/power-cut                        # this checkout's tools/install.sh
sudo tools/testenv/power-cut --script <install.sh>  # another copy of it
```

It puts `$PREFIX` and the impulse-shell config on two file systems (loop devices behind dm-flakey),
cuts the power of one of them at each step of an install and of a rollback, mounts both again
and asks whether `impulse-shell -c impulse-shell` still starts and the next install installs (about 40 s; the
header of the script says what a case is). It needs root, `dmsetup` with the flakey target
and `mkfs.ext4`, which the ri-testenv container does not have: run it on the host after a
change to the order of the steps in `tools/install.sh`. In the gate `install_rollback`
(`each_step_of_a_switch_is_on_the_disk_before_the_next`) holds the order of the `sync` calls.

## pre-commit

`.pre-commit-config.yaml` ([pre-commit](https://pre-commit.com)); once per clone:

```sh
pre-commit install
```

| When | Hook | Needs |
| --- | --- | --- |
| `git commit` that touches `*.rs` | `cargo fmt --check`, both workspaces | rustup (it installs the pinned toolchain) |
| `git push` that touches Rust code or manifests | `tools/ci/gate --workspace --only clippy,shipped-clippy` | Docker |

Clippy runs on push, not on commit: it compiles the workspace (about 50 s after a change to
`crates/qml`, with the cache of an earlier run).
`pre-commit run --all-files --hook-stage pre-push` runs it by hand.

## CI

The workflows run the same steps on GitHub-hosted runners, in an image built from the same
`Containerfile` and published as `ghcr.io/<owner>/<repository>/ri-testenv-<repository id>:<tag>`
(`testenv-image.yml`). A standard runner of a private repository has 2 vCPU, 7 GB RAM and a
6-hour job limit, so the gate is spread over jobs (`gate-jobs.yml`).

| Workflow | Runs on | Jobs: steps |
| --- | --- | --- |
| `checks.yml` | push to `main` and `impulse-next`; by hand | `lint`: fmt, deny. `clippy`: clippy, shipped-clippy. `unit-tests`: tests (`--lib` only), no-interp, shipped-build, shipped-tests. `aot`: aot. |
| `gpu-precision.yml` | push and pull request that touch the GPU code or the image; by hand | gpu-precision. |
| `ri-gate.yml` | by hand | The whole gate: tests in four partitions, `aot`, `clippy`, `shipped`, and `rest` (every other step). |

By hand: Actions → the workflow → Run workflow, any branch.

Why not the gate on every pull request: a private repository on the Free plan has 2,000 runner minutes a
month. `checks` costs about 50 a run; the whole gate, where every job builds its own part on
2 vCPU, several hundred. When the minutes are used up GitHub starts no job at all until the
next month ("The job was not started because ... spending limit needs to be increased"), and
`tools/ci/gate --workspace` is then the only check there is.
