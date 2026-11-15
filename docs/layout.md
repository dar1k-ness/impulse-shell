# Layout

| Path | What is in it |
| --- | --- |
| `crates/shell` | The `impulse-shell` binary: Wayland client, layout, scene, GPU renderer, input, IPC, the `qs` command line. |
| `crates/qml` | QML parser, object model, bindings, signals, JS value model and builtins, and the original's services in Rust (notifications, PipeWire, tray, Hyprland, brightness, ...). |
| `crates/qsg` | Drawing primitives: rectangles, shadows, shapes, text, blur, SVG, images. |
| `crates/rigen` | Compiler from the original's JavaScript to Rust source. |
| `crates/ricompiled-gen`, `crates/ricompiled`, `crates/ricompiled-rt` | Run `rigen` at build time; hold the generated code (32 crates under `ricompiled/parts`) and its registry. |
| `crates/aotcheck` | Compares compiled code with the interpreter on a whole QML tree. Tests only. |
| `crates/bus`, `crates/hypripc`, `crates/workpool`, `crates/elog` | Shared D-Bus connections; Hyprland request socket; worker threads and child reaper; `eprintln!` that drops a failed write to stderr instead of panicking. |
| `crates/colorgen`, `crates/imgtools`, `crates/heif` | Rust ports of the original's color and image scripts; AVIF/HEIC through system libheif, AVIF colours through system libavif when it is installed. |
| `crates/bits` | Build-time check that the flags of a hand-numbered bit set are distinct single bits, and a test that no such flag in `crates/` is declared outside it. |
| `crates/testenv` | Test support: what a test does when its reference tool is missing. |
| `crates/testscenes` | Test support: compiles the QML scenes of the shell's tests (`crates/shell/tests/scenes`) with `rigen`, so they run without the interpreter too. Tests only. |
| `qml` | The shell's QML: what it is built from and tested against, and what an install copies to `~/.config/rustshell/impulse-shell`. |
| `install.sh` | The whole install by one command on Arch Linux: packages, build, install, the user's settings and Hyprland files, the start (docs/install.md). |
| `tools/install.sh` | Build and install. |
| `tools/testenv` | The test image (`Containerfile`) and the gate script (`ri-gate`). |
| `tools/build.sh`, `tools/carry-over.sh` | The build alone; a user's data and own files from the names of the original, or from the shell's previous name, to this project's. |
| `tools/ci` | The gate as run on a checkout (`gate`), the image's tag, the jobs of the workflows. |
| `tools/parity`, `tools/stress`, `bench` | Pixel comparison with the original, stability runs, VM benchmarks. Each has its own README or header. |
| `tools/reference` | Scripts that record what the original, Quickshell, KDE or another GPU does, for the tests that compare with it. |
| `tools/gen` | Generators of the committed tables: locale data, emoji, line breaking, character sets, QML types. |
| `tools/oracle` | Scene files and the PySide6 scripts that render them with Qt: reference images for the renderer. |
| `vendor/regress` | Patched copy of the `regress` crate (`[patch.crates-io]`). |
| `tools/gen/qqmljs` | The grammar of Qt 6.12's QML parser and the script that builds the parser's tables from it (`docs/third-party.md`). |
