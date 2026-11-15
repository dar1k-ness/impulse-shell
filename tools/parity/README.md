# tools/parity: native impulse-shell against the original in Quickshell, shot by shot

Runs every scene of `scenes.sh` under the original (Quickshell running the original's QML) and under
native `impulse-shell` in the same Hyprland guest, compares each pair of shots and says pass or fail. The last full run,
its numbers and what it found are in [REPORT.md](REPORT.md).

It runs in the QEMU guest of `bench/vm.sh` only: the scenes move the pointer, lock the session,
pin the clock and rewrite the shell's config and state. Nothing here is for a real desktop.

## Running it

```sh
bench/vm.sh create                                      # once: the guest (bench/README.md)
bench/vm.sh sync                                        # this checkout -> guest ~/impulse-shell, its `qml` as the impulse-shell config;
                                                        # runs provision.sh: what the scenes need
tools/parity/run.sh                                     # build native in the guest, shoot both shells, compare
tools/parity/run.sh lock polkit                         # the same for some scenes
A=orig B=orig tools/parity/run.sh                       # the original twice: the noise of the tool itself
python3 tools/parity/compare.py A_DIR B_DIR OUT_DIR     # compare two capture directories again
python3 tools/parity/test_compare.py                    # the comparator's verdicts on drawn shots
bash tools/parity/test_shell.sh                         # the shell is followed and stopped by its pid
```

`run.sh` takes `OUT` (result directory on the host, default `/tmp/parity`), `A` and `B` (the two
sides), `BIN` (a native binary already in the guest, under any file name; then nothing is built),
`REV` (the revision that binary must be from, default the host checkout's) and `ORIG` (where the
original is in git history, below).

## The two sides

- Native: `impulse-shell` built in the guest from the checkout, running the `impulse-shell` config
  (`~/.local/share/impulse-shell`), which `bench/vm.sh sync` makes a copy of the checkout's `qml`.
- Original: Quickshell running the original's own QML. That tree is not in the checkout: since #53 `qml` is
  the project's own tree and imports the `Impulse` module, which Quickshell does not have
  (`module "Impulse" is not installed`). `orig.sh` takes it from git history, `ref/ii` of the last
  commit of `main` before #53 (`ORIG`, `REVISION:PATH`), and unpacks it in the guest as
  `~/parity-original`; the original is `qs -p ~/parity-original`. `run.sh` calls it, and so does
  `bench/vm.sh sync`, for the benchmarks that measure against the original. The clone needs that commit (not a shallow
  clone). `orig.txt` in the capture directory says which tree was run.
- So a difference between the two sides is either native drawing the original's QML differently or a place
  where `qml` was changed on purpose since that commit. REPORT.md keeps the two apart.

Exit code of `run.sh` and `compare.py`:

| code | meaning |
|---|---|
| 0 | every shot passed |
| 1 | at least one shot or log differs for real |
| 2 | the run itself is not valid (below), whatever the shots show |

A run is not valid when a shot is missing on one side or has another size, when a shot of a scene
that opens something equals the idle screen of its own run, or when a scene did not put the shell
back to the idle screen. `capture.sh` stops at once (exit 1 from the guest, no comparison) when a
tool is missing, the pointer did not land where it was sent, something that must open or close did
not, or the native build is stale.

Results in `OUT`: `orig/` and `native/` (shots, shell logs, `state.log`, `overlay-actions.log`, `osd-life.log`,
`frames.txt`, `orig.txt` or `build.txt`), `diff/summary.md` (one row per shot with the numbers and the verdict),
`diff/results.json` (the same for scripts) and `diff/<shot>.png` for each shot that differs
(original, native, differing pixels in red).

## The verdict (`compare.py`, `policy.json`, `masks.json`)

- `coarse` is the number a shot is judged by: pixels whose colour still differs by more than 24 of
  255 after both shots are averaged over 5x5 pixels. Two rasterisers put the same glyph or curve
  on slightly different pixels, and the average cancels that; a part that moved by a pixel or two,
  changed colour or is missing stays.
- A shot passes when `coarse` is within its limit in `policy.json`: 0 for a still screen, 12000
  for a frame of a running animation. Each limit has its measured reason next to it.
- `masks.json` lists rectangles left out of the comparison because what they show differs between
  two runs of one shell: live values (CPU, memory), a random icon. One line per
  rectangle with the reason. The lines that start with `renamed` are the names the project changed
  on purpose and the original still shows: the app id and title of the shell's own windows in the
  bar (`org.impulse-shell`, `Impulse Settings`) and the config folder in the Assistant hint of
  the left sidebar. Each covers the differing words and one pixel around them, nothing else of
  the shot.
- `areas` in `policy.json` are rectangles of a shot with limits of their own, for a difference the
  whole-shot number cannot see: a button that is a shade lighter while it is pressed differs by
  less than 24 of 255. Inside the rectangle the mean colour difference and the largest one after
  the 5x5 average each have a limit; the shot differs when either is over it. The limits come from
  the spread between runs of the original, written next to them.
- Animation frames (`NAME.fNN`) are compared with the closest frame of the original, because two
  processes never start an animation at the same instant.
- `state.log` and `overlay-actions.log` (what the scenes read back instead of shooting: the saved
  "Keep awake" state, what the clipboard got, whether the recorder ran, which folder was opened)
  are compared as text.
- `px differ`, `>8`, `>32`, `max` in `summary.md` are raw per-pixel numbers, for reading a diff;
  they do not decide anything.

## What makes two runs equal (`setup.sh`, `lib.sh`)

- Wall clock pinned to 2026-01-15 12:34:30 (`clock.sh`), so clock and date texts are equal. Timing
  inside the tool uses the monotonic clock. `build.sh` and `provision.sh` put the real time back
  first: with the pinned clock cargo and pacman cannot download (TLS).
- The shell's uptime is 1 h 23 min 45 s: `launch.sh` starts the shell in a mount namespace of its
  own, where `/proc/uptime` is a file that says so. The guest's own `/proc/uptime` keeps running
  (the tool times itself by it), and nothing else about the shell's process changes.
- A generated wallpaper and the colours made from it; `panelFamily` the original, horizontal bar, first-run
  file present, a known password for lock and polkit, one audio sink at a known volume.
- The Qt platform theme the end-4 dots use (`kdeglobals.py`): application font, icon theme, caret
  that does not blink.
- the original's Python environment in the Hyprland environment (`IMPULSE_SHELL_VIRTUAL_ENV`): the
  region selector finds content regions with it.
- Every shell start resets what both shells share in `states.json`: sidebar tab, overlay widget
  positions, the Hyprland instance the state belongs to, "Keep awake".
- Before every shell start the entries of `$HOME` get fixed modification times, one minute apart
  in name order: the wallpaper selector lists them by time, and under the pinned clock everything
  a run writes to has the same second. `screenRecord.savePath` is set, so the original's `record.sh` records
  into `~/Videos` and not into `./null`.
- Pointer: an absolute uinput device (`ptr.py`), so it lands on the pixel it is sent to with no
  acceleration; after every move, click and drag the position is read back from Hyprland and a
  miss stops the run. `ptr.py` waits for Hyprland, not for a fixed time: it sends the first event
  once Hyprland lists the device and closes the device once Hyprland has answered a request made
  after the last event (a closed device loses the events not read from it yet).
- A shot is taken when the screen stopped changing (`settle`), not after a fixed delay, and after
  what it shoots is there (`wait_for layer ...`, the session lock, a change of the screen). Stopped
  means one frame for `QUIET_MS` (1 s): an animation stands still for a few tenths of a second in
  the middle when the shell is busy, and two equal screenshots 0.1 s apart took that for the end.
  Shots still changing after 8 s are listed in `unsettled.txt` and marked in `summary.md`;
  `welcome` always is, its wavy slider never stops. The log says for every shot how long `settle`
  looked and the longest pause between two changes it saw (the bar's Resources change every 3 s
  in both shells and nothing else does on the idle screen or with a sidebar open, so a pause of a
  second and more is usually that); its times are ms since the guest booted.
- What is shot has to stay longer than the shot waits. The OSD stays 1 s by default, so `setup.sh`
  sets `osd.timeout` to 5 s in the guest's config for both shells, and the OSD scenes check after
  the shot that the OSD was still up. How long the OSD stays is measured apart, with the original's default
  of 1 s and no shot (`osd_life`: ms from the last event to the OSD layer being gone, the middle
  of 5, in `osd-life.log`), and `compare.py` holds the two runs to `times` in `policy.json`.
- No time tells a first paint that is later than `QUIET_MS` from the end. Where that was seen,
  the typed character on the lock screen, the step is done twice and the shot is the frame both
  times ended on (`shot_twice`); a step that ends differently four times in a row is listed in
  `unsettled.txt`. `bash tools/parity/test_settle.sh` plays the recorded frames of that character
  to `settle` and to the lock scene, without a guest.
- After every scene the screen is compared with the idle screen of the run (`idle_check`); a scene
  that left something open is a tool error, and the shell is restarted so later scenes are clean.

## Scenes

- `firstStart` starts the shell as the first start after a login (`states.json` belongs to another
  Hyprland instance and has "Keep awake" on), then again in the same login: the original turns
  "Keep awake" off in the first case and keeps it in the second.
- `ptr_*` scenes hover, click and drag: sidebars, overview, notifications, overlay widgets and
  every overlay button. `region` opens the region selector over the idle screen.
- `ptr_hold` presses a button and shoots it 1.5 s into the press, still down: the ripple that fills a
  `RippleButton`, which a click does not show (`hold` / `release` in `lib.sh`). `ptr_hold_buttons`
  does the same on a bar button and on the settings window's close button.
- `anim`, `notifburst`: back-to-back frames right after the trigger (`frames.txt` has the times).
  Hyprland's own open animation of the layer is switched off for these (`layer_anim`).
- `bar_vertical`, `bar_bottom`, `bar_float`, `waffle`: `*_live` shots are taken after editing
  `config.json` under the running shell, the others after a restart with that config.
- `settings`, `welcome`, `killDialog`: standalone windows (`qs -p FILE` from the original tree,
  `$BIN -p FILE` from the `impulse-shell` config).

## The native build (`build.sh`)

`build.sh` builds in the guest from `~/impulse-shell` the way `tools/install.sh` does and writes
`REVISION SHA256` next to the binary (`impulse-shell.rev`). `capture.sh native` stops when there is no such
record, when the binary has another checksum, or when the revision is not the one `run.sh` asks
for (the host checkout's): `bench/vm.sh sync`, then run again.

## Gotchas

- Never `pkill -f <pattern>` with a pattern that occurs in your own ssh command line; use
  `pkill -x` or a `[x]yz` bracket pattern.
- The original lock surface needs more than 3 s on llvmpipe before it shows the lock UI.
- Re-adding an overlay widget from its taskbar makes the original's delegates show another
  widget's content, so those toggles go last in `ptr_overlay_buttons` and the two frames it then
  draws wrong are masked on the last three of them.
