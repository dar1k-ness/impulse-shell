# Parity report: native rusty-impulse against the original

What the last full run of the matrix measured, what it found, and what the tool leaves out. How to
run it and how a shot is judged is in [README.md](README.md).

## The run

- Native: `rusty-impulse` built in the guest from `298224946daa` (`build.txt`). That is a local merge of
  `main` at `5121c13f` (#67) with `bb01d676`, the head of #156 before this report was added to it
  (tree `5b0cfe16`); the commit is not in the repository. It runs the checkout's `qml`.
- Original: Quickshell 0.3.2 (Qt 6.12.0) running the original's own QML, `ref/ii` of `c33d2c5f`, the last
  commit of `main` before #53, taken from git history (`orig.txt`). Since #53 the checkout's `qml`
  does not load in Quickshell (`module "Impulse" is not installed`).
- Guest: the QEMU guest of `bench/vm.sh` (Arch, Hyprland 0.57.1, Mesa 26.2.6 llvmpipe, 1920x1080,
  6 CPUs, 16 GB). Nothing ran outside it.
- Matrix: 31 scenes, 137 rows: 93 still shots, 42 frames of 4 animations, 2 logs.

```sh
bench/vm.sh ssh '. ~/parity/lib.sh; real_clock'
bench/vm.sh sync
OUT=$HOME/shots/d9efull tools/parity/run.sh                 # the original against native
A=orig B=orig OUT=$HOME/shots/noise-e tools/parity/run.sh   # the original twice
```

| run | rows | pass | differ | tool errors | exit code |
|---|---|---|---|---|---|
| original against native | 137 | 137 | 0 | 0 | 0 |
| original against original | 137 | 137 | 0 | 0 | 0 |

Every row of both runs is in [results/summary.md](results/summary.md) (against native, with
`results/results.json`) and [results/noise-summary.md](results/noise-summary.md) (the original
twice). No scene left anything open (`idle_check`), no shot of a scene that opens something equals
the idle screen, the two logs are equal on both sides.

## What differs in native: nothing in this run

The run before this report (`main` at `6da570d8`, 136 rows) had 17 rows differing by six causes,
D1 to D6; three more showed up after it. All nine were native drawing the original's QML differently, and
each is fixed in native by its own change with a test against Qt:

| | What was on the screen | Rows | `coarse` then | Fixed by |
|---|---|---|---|---|
| D1 | Overlay, Notes widget: the lines of the placeholder text were 18 px apart in the original and 19 px in native | `ptr_overlay_btn_task5` | 9187 | #76 |
| D2 | Overlay, after the mixer widget is dragged the pointer rests on its corner: an arrow in the original, a resize cursor in native | `ptr_overlay_mixer` | 388 | #79 |
| D3 | Overlay, MangoHud widget switched on: native showed a text caret in "Set FPS limit", the original none | `ptr_overlay_btn_task1` | 80 | #88 |
| D4 | Waffle taskbar: the back rectangle of the Task View icon was one pixel off | 9 `waffle*` rows | 3 to 4 | #85 |
| D5 | Bar, left sidebar open: the highlight of the top-left button was a circle in the original and a taller pill in native | `sidebarLeft`, `ptr_sidebarLeft_anime`, `ptr_sidebarLeft_hover_clear`, `ptr_sidebarLeft_intelligence` | 350 | #83 |
| D6 | Right sidebar, quick toggles in edit mode: the only toggle of the last row was 3 px to the left | `ptr_sidebarRight_editMode` | 237 | #86 |
| D7 | Waffle, first shot after a start: the speaker icon in the taskbar was the muted one in native, in some runs | `waffle` | 23 pixels | #110 |
| D8 | Region selector opened by an overlay button: native also offered the regions of the overlay's widgets, in some runs (S1) | `ptr_overlay_btn_region`, `ptr_overlay_btn_regionRecord` | 32334 | #103 |
| D9 | Waffle start menu: the theme's application icons were one pixel bigger on each side in native | `waffle_startMenu` | 249 | #156 |

D7 and D8 came and went between runs, so one run without them does not show they are gone: their
tests do (`kirigami_icon_repaint_equiv`, `screen_copy_order`), and three more runs of the scenes
`waffle` and `ptr_overlay_buttons` on `main` with #103 and #110 had neither.

### Different on purpose, and not shot

`qml` differs from the original's tree in 256 files. One of those differences is on the way of a
scene: `search toggle` opens the Task View in the original's Waffle (two handlers of one target,
the Task View's wins) and the start menu in native since #93, which gave the Task View its own
target, `search workspacesToggle`. The scene `waffle` opens the start menu by its taskbar button
and the Task View by `search workspacesToggle` (`waffle_startMenu`, `waffle_taskView`), which do
the same in both shells; what `search toggle` itself opens is not compared. The row
`waffle_taskView` is new with that, which is why the matrix has 137 rows and not 136.

### In the log, not on the screen

Native writes `Binding loop detected for property "collapsedWidth"` (`NavigationRailTabArray`)
four times in each of `shell-rusty.log`, `shell-firstStart.log` and `shell-horizontal.log`; the
original writes none. It came with #86 and no shot differs by it. The shells' logs are kept with
the shots and are not compared.

## What passes, and by how much

| | Still shots, `coarse` | Still shots, raw pixels over 8 of 255 | Animation frames, `coarse` |
|---|---|---|---|
| Original against original | 0 on all 93 | 0 on all 93 | up to 740 (`notifburst`), limit 12000 |
| Original against native | 0 on all 93 | 0 on 11, median 429, up to 8948 (`wallpaperSelector`) | up to 9080 (`anim_overview`), limit 12000 |

- Two runs of the original differ on 24 of the 93 still shots outside the masks, by 1 to 284
  pixels and on none of them by more than 8 of 255; the other 69 are equal pixel for pixel. Over
  all the runs of this work the frames of two runs of the original differed by up to 4263,
  native's by up to 9116; a frame that shows another screen measures 100000 and more.
- Native against the original differs on every shot by rasteriser: glyph and curve edges (a few
  hundred pixels over 8), translucent panels a level or two off (up to 31755 pixels differ at
  all, `waffle_taskView`). `coarse` is 0 on all of it, which is why the limit for a still shot is
  0, not a guess.
- `test_compare.py` checks the verdict on drawn shots: equal and noise pass (exit 0), an icon
  moved by two pixels or recoloured fails (exit 1), a missing or wrong-size shot and a shot equal
  to the idle screen are tool errors (exit 2).

## What the tool leaves out or works around

- **S1, region selector from the overlay.** The overlay's region buttons open the selector while
  the overlay closes; the regions it offers depend on which happens first, and the scene cannot
  decide that. Native lost that race in some runs (D8); since #103 its IPC handlers run once the
  compositor applied the caller's window changes. `region` opens the selector over the idle
  screen, where there is no race.
- **S2, first region selection after a start.** The original freezes the screen late on the first
  one, with its own dimming and pointer hint on it, and the hint is at the pointer or at 0,0 by
  chance (two runs of the original differ by 22000 pixels). One selection is opened and closed
  before the ones that are shot (`region_warm`).
- **S3, text caret (fixed, no longer masked).** The original honours `CursorBlinkRate=0` of
  kdeglobals and always shows the caret; native used to blink whatever the setting, so the caret
  was on some of native's shots, and a 3x20 rectangle was masked on `search`, `search_query`,
  `overview`, `ptr_overview*`. Since #88 native reads the theme's cursor flash time
  (`crates/shell/tests/caret_blink_equiv.rs`) and the masks are gone.
- **S4, the original's own bug (masked).** After an overlay widget is switched off and on from the
  overlay's taskbar the original draws the Crosshair widget inside the Recorder frame and the
  Resources widget inside the Crosshair frame. Native does not. Those two frames are masked on
  `ptr_overlay_btn_task4` to `task6`, and the toggles go last in their scene.
- **Live values (masked).** CPU, memory and uptime texts and graphs, the random icon of a critical
  notification, the wavy slider of the welcome window: `masks.json`, one line each with the reason.
  With no cpufreq in the guest the overlay's Resources widget says `of NaN GHz` in the original and
  `of --` in native, inside the same mask.
- **Not covered.** Tray menus (the guest has no tray items) and the cheatsheet's content (empty in
  both shells in the guest).

## The audit's findings in the tool

| | Was | Now |
|---|---|---|
| T1 | The overlay button scene closed the overlay: 9 empty shots, and the overlay stayed open on about 37 later ones | Every button starts from an open overlay and the scene checks which ones leave it open; widget positions and the set of open widgets are reset by a restart after the scene; after every scene the screen must equal the idle screen or the run is not valid |
| T2 | Pointer moved by relative steps through acceleration: drags ended up to 24 px apart | An absolute uinput device; after every move, click and drag the position is read back from Hyprland and a miss stops the run |
| T3 | The region selection scene could not be compared | The selector is shot when its layer is up and the screen stopped changing, after one warm-up selection; both shells get the original's Python environment and find the same regions; `region` adds it over the idle screen |
| T4 | The first start after login was never run | `firstStart`: `states.json` of another Hyprland instance with "Keep awake" on, then a second start in the same login; the saved state is compared as text and the tile is shot |
| T5 | No pass or fail: 0 of 129 identical, exit code 0 | `coarse` with a measured limit per kind of shot; exit 0, 1 (differs) or 2 (run not valid) |
| T6 | `waffle_osd` shot nothing: the original's `osd trigger` calls a function that does not exist | The OSD is shown by a volume change and shot when its layer is up |
| T7 | A stale native build only warned | `build.sh` records revision and checksum; `capture.sh` stops on a missing record, another binary or another revision |
| T8 | This report described an older branch and 79 rows | This file, with the rows of the run next to it |

Found on the way and fixed: the wallpaper selector lists `$HOME` by modification time, and under
the pinned clock the order of the folders was a matter of milliseconds (11 pixels between two runs
of the original); the original's `record.sh` recorded into `./null` because the test config had no
`screenRecord.savePath`; the "Starting recording" popup was on the shot after it.
