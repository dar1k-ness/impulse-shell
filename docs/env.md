# Environment variables (`II_*`, `RI_*`)

Every `II_*` / `RI_*` name in `crates/`, `tools/`, `bench/`, `.github/` and `.config/`, and
what it does. `crates/qml/tests/env_registry.rs` fails when a name, an `RI_DEBUG` switch or
an `RI_LOG` target is in the code but not here, or here but not in the code.

The installed shell (`rusty-impulse` built without the `interp` feature) reads only the variables of
the first three sections. Everything else is read by test and tool builds.

## Shell

| Variable | Read by | What it does |
| --- | --- | --- |
| `RI_LOG` | `qml::debug::init_log` | Which log lines go to stderr: see [RI_LOG](#ri_log-targets). |
| `RI_DEBUG` | `qml::debug::opts` | Comma list of switches and `name=value` parameters: see [RI_DEBUG](#ri_debug-switches). An unknown name prints a warning. |
| `RI_RENDER` | `crates/shell/src/main.rs` | `RI_RENDER=cpu`: no GPU renderer is created, frames are rasterized on the CPU into `wl_shm` buffers. |
| `RI_MODULES` | `crates/shell/src/main.rs` | `RI_MODULES=a.qml,b.qml`: load only these `modules/rusty` files plus the ShellRoot instead of the whole `shell.qml`. |
| `RI_RELOADED` | `crates/shell/src/main.rs` | Set by the shell itself when `Rustshell.reload()` re-executes it: holds the instance id and launch time the new image goes on with. Removed from the environment at startup. Not for people. |
| `RI_HANDED_OVER` | `qml::native` | Set by the shell itself when `Rustshell.reload()` re-executes it: the process groups of the children the old image handed over, which the new one ends. Not for people. |
| `RI_RELOADED_SURFACES` | `crates/shell/src/reload.rs` | Set by the shell itself across a reload's `exec`: the number of a copy of the old image's Wayland socket. While it is open the compositor goes on showing the old image's windows; the new image closes it once its own windows have their first frame and its pictures have loaded and faded in, or after 5 s without that. Removed from the environment at startup. Not for people. |
| `RI_SIGNAL_PIPE` | `crates/shell/src/shutdown.rs` | Set by the shell itself across a reload's `exec`: the inherited signal pipe; a termination signal still unread in it ends the new image at once. Not for people. |
| `RI_LOG_FD` | `crates/shell/src/rilog.rs` | Set by the shell itself across a reload's `exec`: the inherited, still locked `log.qslog` the new image goes on writing. Not for people. |
| `RI_INTERP` | `crates/shell/src/main.rs`, `crates/shell/src/bin/gpushot.rs` | Only in a build with the `interp` feature: `RI_INTERP=1` runs every script in the interpreter instead of the compiled code. The installed shell has no interpreter and ignores it. `gpushot` (a test tool) reads it the same way. |

## Qt's scale variables

Read once at startup, after the config's `//@ pragma Env` and `//@ pragma DefaultEnv` lines
(`crates/shell/src/hidpi.rs`), with Qt 6.12's rules (`QHighDpiScaling`). The programs the
shell starts get the values of the session, not the pragmas'.

| Variable | Read by | What it does |
| --- | --- | --- |
| `QT_SCALE_FACTOR` | `shell::hidpi::Factors::from_env` | A number, fractions too (`2`, `1.5`, `0.5`): every screen's scale is multiplied by it. QML keeps its sizes; windows, margins, exclusive zones, popups and textures are that many times larger on the output, a screen's `width`/`height` that many times smaller, and `devicePixelRatio` that many times higher. Text that is no number (`abc`, `2,5`, `2x`, empty) is 1, without a word. A number no window can have (`0`, a negative one, `nan`, `inf`) is 1 with one warning in the log (Qt differs: [quickshell-differences.md](quickshell-differences.md#qts-scale-variables)). |
| `QT_SCREEN_SCALE_FACTORS` | `shell::hidpi::Factors::from_env` | A factor per screen, separated by `;`: by position in the order of the outputs (`1.5;2`) or by output name (`DP-1=1.5`). A screen without an entry has 1; an entry that is no number above 0 is dropped without a word, so the entries after it move up a screen; an infinite one is dropped with one warning in the log; a factor below 1 is 1. Multiplied with `QT_SCALE_FACTOR`. |

## Build

| Variable | Read by | What it does |
| --- | --- | --- |
| `RI_ROOT` | `crates/ricompiled-gen/build.rs`, `tools/install.sh`, `tools/parity/build.sh` | The QML tree compiled into the binary. Default: `qml` of the checkout, for `tools/install.sh` too, which then makes the rusty-impulse config `~/.config/rustshell/rusty-impulse` a copy of it. The build fails without a tree. |
| `RI_ROOT_SUM` | `crates/ricompiled-gen/build.rs`, `tools/build.sh` | A checksum of the content of the `RI_ROOT` tree, set by `tools/build.sh` (`tools/build.sh --sum <tree>` prints it). Cargo compiles the tree again when this value differs from the one of the last build; without it Cargo goes by file times alone and keeps its build of a tree whose files are older than that build. The build computes the checksum of the files it compiles itself, and `rusty-impulse --version` prints that (`sum`); a value that is not that checksum fails the build. |
| `RI_WORD` | `tools/carry-over.sh`, `tools/pc/rollback-watch` | The word that stands in place of `quickshell` in the project's own names (default `rustshell`). `tools/carry-over.sh` names with it the directories it carries a user's data over to, and stops if it is `quickshell` or not a name; `tools/pc/rollback-watch` takes it as the prefix of the new build's layers and the name of its runtime directory. Set it only to the word the installed build was renamed with. |
| `RI_COMMIT` | `crates/shell/src/ricli.rs`, `crates/shell/src/main.rs`, `tools/build.sh` | The commit `rusty-impulse --version` names. `tools/build.sh` sets it from the checkout (`+changes` when a file differs from the commit or is not in it); for a copy of the sources without `.git` the caller sets it to what `tools/build.sh --commit` prints in the checkout the copy was made from (or the copy's file `.build-commit` holds it, which `bench/vm.sh` writes), and the script fails without it. A release build of the shell without the interpreter does not compile without it; other builds (tests, `--features shell/interp`) say `unknown`. |
| `RI_DIR` | `crates/rigen/check/build.rs`, `tools/testenv/ri-gate` | The QML tree whose scripts the `rigen/check` workspace also compiles. Unset: only its own cases. |

## `RI_DEBUG` switches

`RI_DEBUG=name,name,...`, lower case. Declared in `qml::debug::Switch`. All are off by default.

Self-checks: the shell computes a result the fast way and the slow way and prints a
`ri::check` warning when they differ.

| Switch | What it compares |
| --- | --- |
| `memocheck` | The memoized layout of a window against a full layout. |
| `laycheck` | A partial relayout (only the resized subtrees) against a full layout of the window. |
| `dmgcheck` | The partially repainted back buffer against a full repaint of the frame. |
| `skipcheck` | A draw command skipped as covering no pixel: it must draw nothing. |
| `inccheck` | A layer texture updated in place against the same layer drawn afresh. |
| `dlcheck` | A replayed recorded draw of a subtree against painting the subtree. |
| `plancheck` | The cached creation plan of a component against the instance built by walking its QML. |

Switches that turn one optimization off (A/B measurements, bisecting a bug):

| Switch | What it turns off |
| --- | --- |
| `nolaymemo` | The layout memo: every subtree is measured and placed again. |
| `nolocal` | Partial relayout after a resize: the whole window is laid out. |
| `noremovelocal` | Treating a delegate in its remove transition as placed by nobody. |
| `noviewlocal` | Treating a ListView delegate's resize as local to the view. |
| `nomovinglocal` | Laying nothing out for a step of a displaced transition: the delegate's view is laid out for each. |
| `noctrllocal` | Treating a resize of a Control's contentItem/background as local to the control. |
| `nofresh` | Tracking of items that have not been through a layout pass yet. |
| `nopolish` | Frame pacing of views: a ListView/GridView applies a model change at once, not at its window's next frame. |
| `nofreeze` | Holding back delegate updates of views in hidden windows. |
| `novirt` | Creating a ListView's delegates in chunks as it is scrolled: every row gets a delegate at once. |
| `nodelegfast` | The quick "model and settings unchanged" check of views, Repeaters and Loaders. |
| `nodelegmemo` | The memo of delegators already found up to date. |
| `nodeleglocal` | Reading again only the delegators a change concerns: a destroyed object, a view's laid-out extent and a view's polish make every delegator read again, and a loaded LazyLoader and a view in a hidden window are read on every pass. |
| `noplan` | Creation plans: every instance of a component is built by walking its QML. |
| `nopost` | The recorded post-creation state copied into later instances of a component. |
| `nodefer` | Holding back the bindings of a Behavior's or Transition's animation children until it is enabled. |
| `nolazy` | Creating a Behavior's animation children only once its `enabled` turns true. |
| `nomoved` | Handling an `x`/`y` change as a move: it invalidates draws like any other change. |
| `norastercache` | Reusing the last raster of an unchanged item: it is rasterized again at every paint. |
| `nocanvascache` | Reusing a Canvas's texture until its operations change: it is rasterized again at every paint. |
| `nodl` | Recording and replaying the draws of unchanged subtrees. |
| `noxlate` | Replaying a recorded draw at a new position when only the position changed. |
| `nolcache` | The cache of layer textures: every layer is rendered every frame. |
| `nodamage` | Partial repaint (`EGL_EXT_buffer_age`, swap with damage): the whole surface is redrawn. |
| `noglcache` | The GL state cache: every program/uniform/filter/attrib call is sent. |
| `nodmabuf` | Binding `zwp_linux_dmabuf_v1`: window copies (previews) arrive through shared memory. |

Other switches:

| Switch | What it does |
| --- | --- |
| `dump` | `ipc call ri-debug dump` is answered: it writes the item tree to the instance's run directory. Off, that call is an unknown target. |
| `xflayer` | A scaled, unrotated subtree is drawn through an offscreen layer instead of directly. |
| `wall_anim` | Animation time follows the wall clock instead of advancing once per drawn frame. |
| `unrooted_drop` | A layout-relevant write to an item outside every window drops the clean state of all windows. |
| `lrevall` | Every property write bumps the layout revision, with no exemptions. |

Parameters for logs:

| Parameter | What it sets |
| --- | --- |
| `prof=<ms>` | `ri::prof` prints only entries at or above this inclusive time. |
| `pprof_us=<us>` | `ri::pprof` threshold in microseconds (default 3000). |
| `deptrace=<prop>` | `ri::deptrace` logs what dirties bindings of this property. |
| `val=<prop>+<prop>` | `ri::val` logs only writes of these properties. |
| `val_items=<id>+<id>` | `ri::val` logs every write to these items. |
| `geom=<type>` | `ri::geom` logs geometry changes of items of this type. |

A fault to recover from:

| Parameter | What it does |
| --- | --- |
| `gpuloss=<n>` | The first GPU context of the process is lost (`EGL_CONTEXT_LOST`) at its n-th frame, as after a driver reset. The shell logs it and draws on with a new context, or on the CPU when the lost one never showed a frame (`n` = 1). |

## `RI_LOG` targets

`RI_LOG` is a `tracing_subscriber::EnvFilter` directive list, e.g.
`RI_LOG=ri::stats,ri::keys` or `RI_LOG=ri::ptr=debug`. Lines go to stderr as the bare message.
Warnings (first table) print without `RI_LOG`; `RI_LOG=ri=off` silences them.

Every line the shell logs to stderr (these, `console.log` and the shell's own messages, all
through `crates/elog`) is written with one write(2), text and newline together. When stderr is a
pipe that other threads or processes write to as well (libwayland with `WAYLAND_DEBUG=1`, a
child process), a line of up to `PIPE_BUF` bytes (4096 on Linux) is never mixed with theirs. A
longer line is written in full, never cut, but POSIX lets another writer's bytes land inside it.
A stderr that does not block (`O_NONBLOCK`) and is full loses nothing: the write waits until it
takes output again, as on a stderr that blocks.

A panic is one such write too (`crates/shell/src/panics.rs`): the thread, the place, the message
and, with `RUST_BACKTRACE=1`, the backtrace, in the words of Rust's own report. The same record
is in the instance log (`log.log`, `log.qslog`) as a critical message of the `default` category,
written out before the panic goes on, so an exit or an abort right after it does not lose it. A
panic of the `qs-log` thread itself, which writes those files, reaches stderr only. The record
is on stderr before the hook waits for the files: it waits, with no timeout, for the `qs-log`
thread to answer that it has written them out, so while that thread is stopped in a write (a
full disk, a log that does not take output) the panicking thread waits with it.

| Target | Warning printed |
| --- | --- |
| `ri::anchors` | "Possible anchor loop detected", once per item. |
| `ri::binding` | "Binding loop detected for property", once per binding; a value a property rejected ("Unable to assign", "Non list data"). |
| `ri::layout` | A window's layout did not settle in 3 frames. |
| `ri::qml` | Quickshell's QML warnings: "Setting `width` is deprecated. Set `implicitWidth` instead." for a `PanelWindow`. |
| `ri::check` | A mismatch found by an `RI_DEBUG` self-check. |
| `ri::debug` | An unknown `RI_DEBUG` name; a set `II_*` variable that is no longer read. |
| `ri::qml` | "Could not convert argument": a signal declared in QML was emitted with a value its parameter type does not take. |

| Target | What it logs |
| --- | --- |
| `ri::stats` | Per-frame and per-window counters and timings: frames, layout, uploads, wakeups, screencopy. |
| `ri::frames` | Each frame callback, draw, skipped draw and scale change of a window. |
| `ri::present` | `wp_presentation` feedback: presented or discarded, with time and refresh. |
| `ri::sched` | The main loop's clock and its next deadline before it blocks. |
| `ri::sync` | Each full sync pass (grabs, idle inhibitors, shortcuts). |
| `ri::fade` | Window cross-fades: start, veil surfaces, frame times. |
| `ri::dirty` | Which item made a clean window dirty. |
| `ri::lrev` | What bumped the layout revision: a property write, a reparent, a destroy. |
| `ri::touched` | Each item marked as needing a redraw, with the property written. |
| `ri::touchdbg` | The type chain of each touched item whose cached layers are invalidated. |
| `ri::tracew` | With `ri::stats`: every property write and every Hyprland event and dispatch, with time. |
| `ri::val` | Property writes with their values (narrowed by `val=` / `val_items=`). |
| `ri::deptrace` | What dirties the bindings of the property named by `deptrace=`. |
| `ri::geom` | Geometry changes of items of the type named by `geom=`. |
| `ri::prof` | Time of each binding, handler and delegate update (threshold `prof=`). |
| `ri::jsprof` | Interpreter: self and inclusive time and call count per JS function. |
| `ri::hprof` | Time of each signal handler run. |
| `ri::dprof` | Time of delegate updates: calls, total, pending, candidates. |
| `ri::mprof` | Time of each text or item measure during layout. |
| `ri::pprof` | Time of each item's paint above `pprof_us=`; MultiEffect layer and draw times. |
| `ri::passdbg` | Each layout pass of a window: place time, settle time, local settles. |
| `ri::plandbg` | Creation plans: items, rebuilt values, copied post-creation state per template. |
| `ri::tmpldbg` | Build-time layouts matched to a document's declarations. |
| `ri::view` | Views and Repeaters: model changes, delegates removed and added. |
| `ri::aotstat` | Count of scripts run compiled and interpreted. |
| `ri::aotmiss` | Each script that has no compiled code (interpreter builds). |
| `ri::paint` | Each item painted, when its paint generation changed. |
| `ri::dldbg` | Recorded draws dropped or skipped, and why. |
| `ri::rastdbg` | Each CPU rasterization uploaded as a texture, with its call site and size. |
| `ri::canvas_debug` | Each Canvas paint: operation count, size, errors. |
| `ri::imgdbg` | Each image load: source, source size, fill mode. |
| `ri::dmgdbg` | The damage rectangles of a frame and reordered draw lists. |
| `ri::dmgdump` | With `dmgcheck`: writes the partial and the full repaint of a mismatching frame as PPM files to the instance's run directory (first 3 frames). |
| `ri::scdump` | Writes each finished screen copy as `sc-<item>.png` to the instance's run directory. |
| `ri::gpu` | Layers and textures scaled down to the GL texture size limit. |
| `ri::gpuprof` | Time of each GPU command and layer render in a frame. |
| `ri::gputime` | Per frame: buffer age, acquire, execute and finish times, command count. |
| `ri::cmdstat` | Per frame: command, triangle, vertex and rectangle counts. |
| `ri::glcount` | Per interval: GL calls by kind (texture uploads, FBO binds, program and uniform changes; `middraw` is the uploads into a glyph atlas made between the draws of a frame). |
| `ri::glerror` | Release builds: read and log `glGetError` after each frame (debug builds always do). |
| `ri::wstat` | Object counts by type and owner, GC runs, jemalloc heap sizes, cache sizes. |
| `ri::keys` | Keyboard focus: target window, focus chain, keyboard mode changes, enter/leave. |
| `ri::ptr` | Pointer: input regions, presses, hits, grabs. Motion events are trace level: `ri::ptr=debug` leaves them out. |
| `ri::grab` | `hyprland_focus_grab_v1`: grabs created, their windows, cleared by the compositor. |
| `ri::appdebug` | xdg_toplevel windows (ApplicationWindow, Window, FloatingWindow): size, configure events, window method calls left unhandled. |
| `ri::ipc` | An IPC client that started listening to a signal. |
| `ri::cava` | The visualizer's audio capture: started, stopped because nothing is on screen (session locked, or no output on). |
| `ri::sni` | System tray: StatusNotifier items, own watcher, menu fetches and events. |
| `ri::pw` | PipeWire: node list, defaults, each node's volume and mute. |

## Former `II_*` diagnostics variables

No longer read. Each was replaced by the `RI_DEBUG` switch or `RI_LOG` target of the same
name in lower case (`RI_NODL` → `RI_DEBUG=nodl`, `RI_STATS` → `RI_LOG=ri::stats`; only
`RI_TOUCH` differs: `ri::touched`). One that took a value also became an `RI_DEBUG` parameter
(`RI_PROF=5` → `RI_LOG=ri::prof RI_DEBUG=prof=5`). The full list is `qml::debug::LEGACY`; the
shell prints a warning naming the replacement when one of them is set.
`tools/reference/equiv/debugenv.sh` compared the old and the new settings.

## Tests and tools

Set by a person or by CI:

| Variable | Read by | What it does |
| --- | --- | --- |
| `RI_ROUNDS` | `crates/qml/tests/notifications.rs` | How many rounds of 500 notifications `flood_500` sends (4 without it). |
| `RI_EQUIV_SKIP` | `crates/testenv` | Exactly `1`: a test whose reference tool, service or compositor is missing prints a skip instead of failing. |
| `RI_REQUIRE_GL` | `crates/shell/src/gpu.rs` tests, `crates/shell/src/gpu_precision_tests.rs`, `tools/testenv/ri-gate` (steps `tests`, `shipped-tests` and `gpu-precision`) | Set: GPU tests fail when they cannot make a GL context (surfaceless EGL, or EGL on the Wayland display), instead of returning early. |
| `RI_SHIPPED_BIN` | `crates/shell/tests/shipped_first_frame.rs`, `crates/shell/tests/shipped_panels.rs`, `tools/testenv/ri-gate` (step `shipped-tests`) | The rusty-impulse binary the first-frame and panel tests run (default: the one cargo built for the test). |
| `RI_SHIPPED_TREE` | `crates/shell/tests/shipped_first_frame.rs`, `crates/shell/tests/shipped_panels.rs`, `tools/testenv/ri-gate` (step `shipped-tests`) | The QML tree they run (default: `ricompiled::ROOT`). |
| `RI_GATE_DIR` | `tools/ci/gate`, `tools/ci/in-testenv` | Directory that keeps the gate's target dir and Cargo registry between runs. Default: `~/.cache/ri-gate` (`$XDG_CACHE_HOME/ri-gate`); in CI the runner's temp dir. See [ci.md](ci.md). |
| `RI_GATE_LOCKED` | `tools/ci/gate`, `tools/ci/in-testenv` | Set by `tools/ci/gate` once it holds the lock of `RI_GATE_DIR`: `tools/ci/in-testenv` then does not take that lock again. Started on its own, `in-testenv` takes it. |
| `RI_PORTS_WAYLAND` | `qml::ports` tests, `ports_ab` | Set (the testenv image sets it): the tests may use the compositor on `WAYLAND_DISPLAY` (clipboard, screenshots). |
| `RI_PORTS_BENCH` | `qml::ports::tests::bench_ports`, `tools/reference/ports-bench.sh` | `orig` or `port`: run every operation as the shell command the original ran or through `qml::ports`. |
| `RI_PORTS_N` | same | Repetitions per operation (default 20). |
| `RI_PORTS_ONLY` | same | Run only this operation. |
| `RI_PORTS_SKIP` | same | Comma list of operations to leave out. |
| `RI_PORTS_VKBD` | `qml::ports::tests::vkbd_sequence`, `tools/reference/ports-keys.sh` | Set: send the paste and on-screen-keyboard key sequences through the virtual keyboard. |
| `RI_BRIGHTNESSCTL` | `qml::brightness::bench` | Path of a `brightnessctl` built with `BCTL_SYS`; unset, the benchmark returns. |
| `RI_INI_CORPUS` | `qml::ini` tests | Colon-separated directories of extra key files to parse. |
| `RI_MICROTEX_DIR` | `files_equiv` | MicroTeX directory (default `/opt/MicroTeX`). |
| `RI_REF_QS` | `qs_list_log`, `config_lookup_equiv` | The Quickshell binary to compare against (default `quickshell` on `PATH`). |
| `RI_IACT_QS` | `crates/shell/tests/interact.rs` | The `qs` binary to compare against (default `qs` on `PATH`). |
| `RI_IACT_NATIVE` | same | The native binary under test (default this build's `rusty-impulse`). |
| `RI_AB_KEEP` | `ports_ab` | Set: keep the test's scene directory after it ends. |
| `RI_TEXT_QT_RENDERING_DUMP` | `text_qt_rendering` | Directory the test writes its two pictures to, Qt's and ours, as raw RGBA. |
| `RI_TEXT_QT_VIEW_DUMP` | `text_qt_view` | Directory the test writes its pictures to, Qt's and ours at every step, as raw RGBA. |
| `RI_SHELL_SCENES_DUMP` | `shell_scenes_qt` | Directory the test writes its two frames of every scene to, Qt's and ours, as raw RGBA. |
| `RI_DUMP_DIR` | `text_dpr_qt` (`crates/shell/tests/support/qt_window.rs`) | Directory the test writes its two pictures to, ours and Qt's, as PPM files. |
| `RI_DUMP` | `riflood`, `tools/reference/equiv/debugenv.sh` | File `riflood` writes its final world dump to. |
| `RI_BIN` | `bench/first-open.sh` | The rusty-impulse binary the benchmark starts (default `rusty-impulse` on `PATH`), e.g. another release under `/usr/local/lib/rusty-impulse/releases`. |

Set by a test for the child process it starts; setting them by hand does nothing useful:

| Variable | Test |
| --- | --- |
| `RI_AB_ROOT`, `RI_AB_INPUT`, `RI_AB_REFERENCE` | `ports_ab`: scene directory and input of the child side; set on the side that runs the reference fixture (real bash and tools). |
| `RI_AB_CASE`, `RI_AB_BASE`, `RI_AB_LOG`, `RI_AB_WANT` | `snip_picker_equiv`: case, directories and expected count of its child. |
| `RI_AUDIO_SIDE_LOG`, `RI_AUDIO_SIDE_NAME`, `RI_AUDIO_SIDE_THEME`, `RI_AUDIO_SIDE_WANT` | `sound_qalc_gcloud_booru_pin_equiv`: its `audio_play_side` child. |
| `RI_LEAF_LOG`, `RI_CTL` | `script_ports_equiv`: log file and control directory of its fake commands. |
| `RI_TEST_HELPER` | `qml::native` tests: name of the helper test the child runs. |
| `RI_SITE_BIN`, `RI_SITE_OUT`, `RI_SITE_MARK` | `userenv_sites` tests of `qml` and `colorgen`: the directory of their stand-in programs, the directory those write to, and the variable they report: `user` in the environment the programs must get, `shell` in the test's own. |
| `RI_USERENV_A`, `RI_USERENV_B`, `RI_USERENV_C` | `workpool::userenv` test: variables of the environment it captures and of the commands it starts. |
| `RI_LAUNCH_PATH_CHILD` | `qml::native` tests: marks the child started as the installed name (the release's link, or the script that is there while releases are switched). |
| `RI_TEST_INSTALL` | `install_rollback` test: the binary its fake `flock` installs while a rollback waits for the lock. |
| `RI_TEST_PATH`, `RI_TEST_FAIL_AT`, `RI_TEST_KILL_AT`, `RI_TEST_SIGNAL`, `RI_TEST_GOES_ON`, `RI_TEST_PID`, `RI_TEST_KILLED`, `RI_TEST_STEPS` | `install_rollback` test: for its stand-in `ln`, `mv`, `tar` and `sync`: the real `PATH`, the call that fails, the call that sends the installer a signal, that signal, whether that call then runs (not set: it fails), the installer's pid (the stand-in's parent can be a subshell of it), the file that marks it sent, and the file they log their command lines to. |
| `RI_TEST_PAUSE_AT`, `RI_TEST_WAITING`, `RI_TEST_RESUME` | `install_rollback` test: for the same stand-ins: the call that waits, the line it prints when it does, and the FIFO it reads before it goes on. |
| `RI_TEST_LOGNAME` | `install_rollback` test: the login name its stand-in `logname` prints. |
| `RI_MKDIR_UMASK_CHILD` | `qml::files` test: umask of its child. |
| `RI_HEAP_TEST_CHILD` | `shell::heap` test: marks the child that measures RSS alone. |
| `RI_IPC_EMFILE_TEST_CHILD` | `shell::ipc` test: marks the child that lowers `RLIMIT_NOFILE`. |
| `RI_PANIC_TEST_RUN_DIR` | `panic_whole`: marks the child that panics, and is the run directory of its instance log. |
| `RI_PANIC_ENDS`, `RI_PANIC_ENDS_RUN_DIR` | `panic_ends`: what its child does (a `+`-separated list of `hook`, `closed`, and `worker` or `main`), and the run directory of the child's instance log. |
| `RI_TEST_PRAGMA` | `startup_errors`: the name its `//@ pragma Env` line sets. |
