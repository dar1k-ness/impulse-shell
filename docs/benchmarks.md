# Benchmarks and live measurements

Three kinds of timing code, all kept out of `cargo test` so a gate run never depends on the
host or on wall-clock noise: criterion micro-benchmarks, live tests against the original,
and the memory and CPU of the whole shell against Quickshell.

## Benchmarks (criterion)

Reproducible micro-benchmarks live in `benches/` and use
[criterion](https://bheisler.github.io/criterion.rs/book/). The gate does not run or build
them: its `clippy` step lints them with every other target (`--all-targets`), the `qml` one
in every run and the `qsg` one only with `--workspace`. To check that they build (the
workspace needs `shell/interp`, as clippy does: without it feature unification turns on
`qml/interp` and the release shell refuses to compile):

```sh
cargo bench --workspace --features shell/interp --no-run
```

Run them (results and HTML reports go to `target/criterion/`, later runs compare against the
previous one):

| Command | What |
| --- | --- |
| `cargo bench -p qsg --bench raster` | sidebar RectangularShadow; SVG load and load+render of every `qml` asset; an Image stretched smoothly and the wallpaper (`image/`); the lock screen's GaussianBlur (`blur/lock`) |
| `cargo bench -p qml --bench json` | `JSON.parse` / `JSON.stringify` of a 40-window `hyprctl -j clients` |

## Live measurements and host checks (`--features live`)

Tests that time the original (its processes, scripts, D-Bus calls) against the native port,
or that read the real host (NetworkManager, every icon under `/usr/share/icons`, a PySide6
QJSEngine), are `#[cfg_attr(not(feature = "live"), ignore = ...)]`: `cargo test` lists them as
ignored with the reason, and the `live` feature of `qml` / `qsg` runs them. Each test's doc
comment has its exact command and the environment it needs. Use `--release` and
`-- --nocapture` for the printed numbers, and run one at a time:

```sh
cargo test --release -p qml --features live --test session -- --nocapture
```

| Test | Needs |
| --- | --- |
| qml `ai_scripts`, `fprint`, `fswatch`, `http_equiv`, `keyring`, `session` (tests) | the testenv tools (`tools/testenv/Containerfile`) |
| qml `hyprland_qml` storm | nothing (a fake Hyprland in the test) |
| qml `hyprconf_equiv` real_file | `HYPRCONF_FILE=<shellOverrides/main.lua>` (read-only) |
| qml `wall_bench`, `gcloud_bench`, `trans_bench` (lib) | `IMPULSE_SHELL_VIRTUAL_ENV` for gcloud; `--test-threads=1` |
| qml `json::tests::ignored_matches_qjsengine` (lib) | PySide6 6.12 |
| qml `brightness::bench` (lib) | `RI_BRIGHTNESSCTL=<brightnessctl built with BCTL_SYS>` |
| qml `ports::tests::bench_ports` (lib) | `tools/reference/ports-bench.sh <test binary>` (a compositor, cliphist, grim, ImageMagick, ydotoold) |
| qml `ports::tests::vkbd_sequence` (lib) | `tools/reference/ports-keys.sh <test binary>` (Hyprland, ydotoold, `wev`) |
| qml `scope_lookup_bench` (test) | nothing |
| qsg `svg::equiv::all_system_icons_identical` (lib) | icons under `/usr/share/icons` |

The other ignored tests are helper processes that a test re-executes itself
(`ports_ab::ab_child`, `snip_picker_equiv`'s child, `native.rs` child helpers; running them
directly does nothing) and `shell::gbm_equiv`, which needs a GPU's DRM render node. Every
`#[ignore]` names its reason (clippy's `ignore_without_reason`), and `tools/ci/skipped-tests`
lists them all; the gate prints that list.

## Shell memory and CPU against Quickshell (`bench/rss-cpu.sh`)

Resident memory and CPU time of `impulse-shell -c impulse-shell` and of the original in Quickshell
(`qs -p ~/parity-original`), in three states: idle, both sidebars open, and a wallpaper switch.

### Run it

Needs a Linux host with KVM, `qemu-system-x86_64`, `qemu-img`, `cloud-localds`, `ssh`, `curl`.
Nothing is installed on the host and no desktop session of the host is used.

```sh
bench/vm.sh create                      # once: the guest (about 5 minutes)
bench/vm.sh sync                        # this checkout -> guest, build + install impulse-shell (6 to 10 minutes the first time); original -> guest
bench/vm.sh ssh impulse-shell/bench/rss-cpu.sh  # about 25 minutes; prints the table below
bench/vm.sh stop
```

Results stay in the guest under `~/rss-cpu/<date>`: `summary.md` (the table), `phases.tsv` (one
line per run and state), `samples.tsv` (one line per second), `env.txt` (versions), and per
run the shell's log and per state a screenshot and the process list of the shell's cgroup.

### Method

| | |
| --- | --- |
| Guest | Arch Linux cloud image `20261101.620452` (SHA256 checked) under QEMU/KVM: 6 vCPU, 16 GiB, one virtio-vga output at 1920x1080. Every package comes from the Arch Linux Archive snapshot `tools/testenv/Containerfile` names, so Quickshell, Qt and Hyprland are the versions of the test image. |
| Rendering | Mesa llvmpipe: the guest has no GPU, so both shells render on the CPU. |
| Compositor | Hyprland with `bench/hyprland.lua`: animations, blur and shadows off, no windows open. |
| Trees | `impulse-shell` runs the checkout's `qml`: `tools/install.sh` compiles it into the binary (`cargo build --release -p shell`) and leaves the same files in `~/.local/share/impulse-shell`; the script refuses to run if the two differ. Quickshell cannot load that tree (it imports the `Impulse` module), so it runs the original: `ref/ii` of the last commit of `main` before #53, the tree `tools/parity` compares the view with. `vm.sh sync` unpacks it from git history as `~/parity-original` (`tools/parity/orig.sh`); `env.txt` names it. |
| Start | Per run: stop both shells, delete the original's state, cache and configuration, write a configuration that only sets the first wallpaper (plus the matugen configuration `switchwall.sh` needs), start the shell from Hyprland inside a systemd scope (its own cgroup), wait until it answers IPC and its bar and background layers exist, then wait `SETTLE` = 30 s. |
| idle | Bar and background only. 60 samples, one per second. |
| panels | `ipc call sidebarLeft open`, `ipc call sidebarRight open`, wait for both layers, 5 s, then 60 samples. |
| wallpaper | Close both sidebars, 5 s, then `ipc call wallpapers apply <second wallpaper>` and 60 samples; the window starts just before the request. The run fails unless the configuration names the new wallpaper and `colors.json` and `material_colors.scss` were generated. Both wallpapers are 3840x2160 JPEGs made by ImageMagick from a fixed seed. |
| Order | `ROUNDS` = 3 runs per shell, alternating: impulse-shell, Quickshell; Quickshell, impulse-shell; impulse-shell, Quickshell. The three states of a run are measured in that order in one process. |
| RSS | `VmRSS` of the shell process from `/proc/<pid>/status`; `RssAnon` is its anonymous part (heap), the rest is mapped files and shared memory. Per run: the median of the 60 samples. |
| RSS with helpers | Sum of `VmRSS` over every process in the shell's cgroup. Pages two processes share are counted twice. |
| CPU shell | `utime + stime` of the shell process (`/proc/<pid>/stat`, fields 14 and 15) at the end of the window minus at its start, in seconds (100 ticks per second). The percentage is that over the window's length: 100 % is one core busy. |
| CPU with helpers | `usage_usec` of the cgroup's `cpu.stat`, the same difference: also counts processes the shell started that already exited. |

The table gives, per shell and state, the median of the three runs and in brackets the
smallest and the largest run.

### Results

This run is older than the Trees row above: at `3aedd70f` the checkout's `qml` was still the
original, and Quickshell loaded it too (`qs -c ii`), so both shells ran the same files.

2026-11-09, `impulse-shell` built from `main` at `3aedd70f`. Quickshell 0.3.2, Qt 6.12.0, Hyprland
0.57.1, Mesa 26.2.6 (llvmpipe, LLVM 23.1.4), Linux 7.2.14, matugen 4.2.0. Host: an 8-vCPU
Intel Xeon cloud VM (so KVM is nested), Ubuntu 22.04, QEMU 6.2.0.

| State | Shell | RSS, MiB | RssAnon, MiB | RSS with helpers, MiB | CPU shell, s | CPU with helpers, s | CPU shell, % of a core |
| --- | --- | --- | --- | --- | --- | --- | --- |
| idle | impulse-shell | 301 (299–302) | 171 (168–171) | 327 (324–328) | 0.15 (0.12–0.30) | 0.14 (0.13–0.29) | 0.25 (0.20–0.47) |
| idle | quickshell | 986 (968–1018) | 817 (800–843) | 995 (978–1027) | 0.59 (0.57–0.61) | 0.58 (0.56–0.61) | 0.97 (0.94–1.00) |
| panels | impulse-shell | 322 (321–324) | 186 (186–188) | 347 (346–349) | 0.10 (0.09–0.32) | 0.10 (0.09–0.32) | 0.16 (0.15–0.50) |
| panels | quickshell | 1201 (1196–1215) | 1019 (1018–1030) | 1210 (1205–1224) | 0.66 (0.66–0.68) | 0.66 (0.66–0.67) | 1.08 (1.05–1.11) |
| wallpaper | impulse-shell | 327 (326–329) | 194 (193–198) | 352 (352–355) | 1.27 (1.22–1.28) | 1.28 (1.22–1.28) | 2.03 (2.00–2.08) |
| wallpaper | quickshell | 1006 (985–1007) | 841 (814–843) | 1015 (994–1016) | 1.50 (1.49–1.73) | 4.40 (3.88–5.79) | 2.44 (2.29–2.83) |

By the medians impulse-shell holds 3.3 times less memory than Quickshell idle (301 against 986 MiB),
3.7 times less with both sidebars open and 3.1 times less after the wallpaper switch. Over a
60 s window it uses 0.15 s of CPU idle against 0.59 s, and 0.10 s against 0.66 s with the
sidebars open. The wallpaper switch costs the impulse-shell process 1.27 s against 1.50 s; with the
processes each shell starts for it, 1.28 s against 4.40 s.

How to read the numbers:

- They are from a VM without a GPU. llvmpipe keeps its buffers in the process and rasterizes
  on the CPU, for both shells, so on a PC with a GPU both RSS and CPU are different numbers.
  This run says nothing measured about a real desktop.
- The desktop is empty: no windows, no notifications, no media player, no Bluetooth, no
  battery. Idle is the bar (clock, resource indicators) and the background with its clock.
- In the wallpaper state most of the CPU time falls in the first two seconds after the
  request (impulse-shell: 1.0 to 1.1 s of 1.2 to 1.3 s; Quickshell with helpers: 3.3 to 5.1 s of 3.9
  to 5.8 s). Compare the seconds, not the percentage.
- The only helper alive at the end of every state is `ri-childguard` (25 MiB) for impulse-shell and
  `nmcli monitor` (9 MiB) for Quickshell.
- The wallpaper state is measured after the sidebars are closed again: Quickshell's RSS is
  back from 1201 to about 1006 MiB, impulse-shell stays at 322 to 327 MiB.
- One tick is 10 ms: the idle and panels CPU numbers are 9 to 68 ticks per window.
- In round 2 five consecutive windows (Quickshell's panels and wallpaper, all three of impulse-shell)
  took 63 to 66 s instead of 61 s, and impulse-shell used 0.30 s and 0.32 s there against 0.09 to
  0.15 s in rounds 1 and 3. Why the guest was slower then was not found; the brackets
  include that round.

Every run:

| Run | State | Window, s | RSS median, MiB | RSS max, MiB | RssAnon, MiB | RSS with helpers, MiB | CPU shell, s | CPU with helpers, s |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| impulse-shell 1 | idle | 61.1 | 302 | 303 | 171 | 328 | 0.15 | 0.14 |
| impulse-shell 1 | panels | 61.0 | 324 | 324 | 188 | 349 | 0.09 | 0.10 |
| impulse-shell 1 | wallpaper | 61.0 | 327 | 335 | 193 | 352 | 1.22 | 1.22 |
| quickshell 1 | idle | 60.9 | 1018 | 1021 | 843 | 1027 | 0.57 | 0.56 |
| quickshell 1 | panels | 61.1 | 1215 | 1286 | 1030 | 1224 | 0.66 | 0.66 |
| quickshell 1 | wallpaper | 61.1 | 985 | 991 | 814 | 994 | 1.49 | 3.88 |
| quickshell 2 | idle | 61.1 | 968 | 971 | 800 | 978 | 0.61 | 0.61 |
| quickshell 2 | panels | 62.8 | 1196 | 1250 | 1018 | 1205 | 0.66 | 0.66 |
| quickshell 2 | wallpaper | 65.6 | 1007 | 1016 | 843 | 1016 | 1.50 | 5.79 |
| impulse-shell 2 | idle | 63.7 | 301 | 302 | 171 | 327 | 0.30 | 0.29 |
| impulse-shell 2 | panels | 64.6 | 321 | 321 | 186 | 346 | 0.32 | 0.32 |
| impulse-shell 2 | wallpaper | 63.0 | 329 | 332 | 198 | 355 | 1.28 | 1.28 |
| impulse-shell 3 | idle | 61.0 | 299 | 301 | 168 | 324 | 0.12 | 0.13 |
| impulse-shell 3 | panels | 61.1 | 322 | 322 | 186 | 347 | 0.10 | 0.09 |
| impulse-shell 3 | wallpaper | 61.1 | 326 | 332 | 194 | 352 | 1.27 | 1.28 |
| quickshell 3 | idle | 61.1 | 986 | 991 | 817 | 995 | 0.59 | 0.58 |
| quickshell 3 | panels | 61.2 | 1201 | 1248 | 1019 | 1210 | 0.68 | 0.67 |
| quickshell 3 | wallpaper | 61.0 | 1006 | 1015 | 841 | 1015 | 1.73 | 4.40 |
