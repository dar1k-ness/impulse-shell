# Parity-kept flaws: waffle, overlay widgets, on-screen keyboard, `-p`, equivalence tests

## Fixed

| Where | Original flaw the port copied | Now | Regression test |
|---|---|---|---|
| FloatingImage (`ImageDownloaderProcess`, `jobs.rs`) | `curl -sSL` without `-f` saved a 404/500 page as the image, and the `[ -f F ]` cache kept it forever: the widget never showed the image again | HTTP error status saves nothing (exit 22); a cached file that is no image is fetched again | `http_equiv::image_downloader_skips_error_pages_and_reports_real_size` |
| FloatingImage size | The QML takes the first `\d+ x \d+` of `file F`; for JPEGs `file` prints `density 72x72` before the size, so JPEG overlays came out 72x72 | The description is led by the real `W x H` (header parse), the regex finds the image size | same test (1920x1080 JPEG with 72 dpi density) |
| FPS limiter (`screen.rs`) | `echo >>` into a missing `~/.config/MangoHud` failed, so no limit was saved | the directory is created | `ports::tests::fps_limit_fixes_original_flaws` |
| FPS limiter | `>>` onto a config whose last line had no newline glued `fps_limit=` onto it | appended on its own line | same |
| FPS limiter | sed's `.*` stopped at the first non-UTF-8 byte and left the rest of the old value | the whole value is replaced (`\r` line endings kept) | same |
| FPS limiter | `sed -i` replaced a symlinked `MangoHud.conf` (dotfile managers) with a regular file | the link target is updated atomically, mode/owner kept | same |
| Resources overlay / ResourceUsage | no cpufreq (VMs, some ARM) made the max-frequency label `NaN GHz` | the label keeps its `--`; readings format as before | `ports::tests::cpu_max_freq_label_skips_nan`, `probe_qml` expectation switched from `NaN GHz` to `--` |
| Equivalence tests | `cover_art_job_runs_for_its_declaration` / `cover_art_file_urls_match_curl` declared `command: ["false"]`, so since jobs only replace their original command they ran `false` and failed | they declare the original command, so they test the native job again | the tests themselves |
| Equivalence tests | `fps_limit_matches_bash` asserted the four sed/echo flaws above byte for byte; `probe_qml` asserted `NaN GHz` | those cases now assert the intended result; the rest still compare against bash | as above |

## Left as is, and why

| Where | Why |
|---|---|
| On-screen keyboard (`vkbd.rs`) layout | The virtual keyboard uploads the seat keymap once, so a keymap changed later is not followed. Following it needs `wl_keyboard` keymap/modifier events, which the compositor only sends to the focused client (the OSK never has focus); the original `ydotool` (uinput) has the same blind spot. Already better than the original: one connection instead of a `ydotool` process per key, and release-all sends only held keys instead of 249 `ydotool key` events |
| Recorder "full screenshot" | Same order as the original (close overlay, then capture), done in-process (screencopy + wl-copy) without `bash`/`grim`. Whether the closing overlay can still be in the frame was not reproduced headless; no change without evidence |
| Recorder record/region buttons, waffle `qs -p … ipc call` buttons | Already native (self-IPC instead of spawning a second `qs`; record script port) |
| Waffle `bash -c Config.options.apps.*` launchers | user-configured shell command lines; running them through a shell is the feature |
| Crosshair overlay | pure QML drawing; no process, polling or file work to fix |
| Resources overlay polling | 1 s `/proc` sampling drives the visible graphs; already read natively without processes |
| `rusty-impulse -p` | parsing and standalone load audited, no flaw found in this area (SpinBox/ScrollView handled elsewhere) |
