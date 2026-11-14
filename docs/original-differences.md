# Deliberate differences from the original

`qml` began as the QML of illogical-impulse ("ii", end-4/dots-hyprland) and does what the original's
tree did. The cases below are the ones where it does something else on purpose, not as a fix
of something the original meant to do. Each names the test that holds the difference. What rusty-impulse does
differently from Quickshell with the same QML is in
[quickshell-differences.md](quickshell-differences.md).

## `ThumbnailImage` shows a thumbnail and never makes one

the original's `ThumbnailImage.qml` can make its own thumbnail: with `generateThumbnail` (true unless set)
it runs `magick` through `bash` each time `sourceSize` changes. Here the file has no such
property and no process.

| | the original | Here |
|---|---|---|
| Who uses the widget | `WallpaperDirectoryItem.qml` only, with `generateThumbnail: false`. | The same file, without that line. |
| Who makes the thumbnails | `Wallpapers.generateThumbnail()`, for the whole directory, when the wallpaper selector opens or changes directory. | The same function, through the native `DirectoryThumbnails`. |
| The widget's own generation | Declared, never runs: its one user turns it off. | Not in `qml`; the native type that stood for its process (`ThumbnailGenerator`) is gone too. |

Why: no user of the original could reach that code, and files and types that nothing reaches are not
shipped. A widget that needs its own thumbnail has to bring the generation back.

Tests: `crates/shell/tests/thumbnail_cyrillic_dir.rs` shows the widget with thumbnails
`DirectoryThumbnails` made; `thumbnail_writer_fixes_the_original` in `crates/qml/tests/files_equiv.rs`
holds the writer both went through.

## An agent of the assistant chat that was asked to end and has not exited

The original has no chat with an agent. This one came with the tree `qml` began as, where a restart sent
SIGTERM and started the new agent 300 ms later if the old one had exited by then.

| | That tree | Here |
|---|---|---|
| Asking an agent to end | SIGTERM. | ACP's order: `session/close` if the agent closes sessions, and after its answer the agent's stdin is closed and it gets SIGTERM. |
| The agent is still running | The header says "Connecting...". A prompt sent then stays in the queue, with no line about it and no end. | The chat has a line "Waiting for `<program>` to exit" with a button "End now", and the header says the same. A prompt sent then says "In the queue" and goes to the agent started after the exit. |
| The agent never exits | Nothing more happens until the shell is reloaded. | "End now" kills it (SIGKILL), and the new agent starts from that exit. Nothing is timed. |

Why: an agent that ignores SIGTERM, or leaves `session/close` unanswered, would otherwise hold
the chat for good without a word.

Code: `end`, `terminate`, `awaitExit` and `kill` in `qml/services/assistant/AcpSession.qml`;
the line in `qml/modules/rusty/sidebarLeft/assistant/AssistantMessage.qml`.

Tests: `crates/shell/tests/assistant_session.rs` (`agent_that_does_not_end_when_asked_is_killed_by_the_button`,
`agent_is_not_ended_before_it_answers_session_close`,
`agent_that_closes_sessions_has_its_session_closed_before_it_is_ended`,
`restart_starts_the_new_agent_when_a_slow_one_has_exited`) and
`crates/shell/tests/assistant_ending_line.rs`.

Not checked: a real agent, and the line on screen.

## Night light that is not on the schedule, as the shell starts

With `light.night.automatic` off, the original leaves hyprsunset as it finds it: a temperature set before
the shell was reloaded stays on the screen while the night light toggle may show it off
(end-4/dots-hyprland #3373, open there).

| `light.night.automatic` | the original | Here |
|---|---|---|
| on, started at night | `temperature 5000` | The same. |
| off | Nothing is sent. | `temperature 6000` once the config file is read, and `load()` sets the gamma back to 100. |

Why: with the schedule off nothing else would ever reset a temperature left from before the
reload.

Code: `onConfigReadyChanged`, `ensureState` and `load` in `qml/services/Hyprsunset.qml`.
`onConfigReadyChanged` reads the option from `Config`, not `automatic`: a change handler runs
before the bindings that read the changed property, so `automatic` is still false there.

Tests: `crates/qml/tests/hyprland_qml.rs`: `hyprsunset_at_startup_like_qt` (both rows, and
that Qt 6.12 sends the same for the same file), `hyprsunset_manual_mode_resets_stale_state`
(`load()`).

Not checked: a real hyprsunset.

## The wallpaper folder after a path that is neither a folder nor a file

`Wallpapers.setDirectory(path)` asks a `test -d` / `test -f` process what the path is.

| | the original | Here |
|---|---|---|
| When the folder changes | At once, to the path as asked, before the answer. | When the answer is "folder" (to the path) or "file" (to the file's folder). |
| The path is neither (a dangling link, a missing path) | The folder stays that path until the next valid one. | The folder stays what it was. |
| The path is a folder or a file | The folder, or the file's folder. | The same. |

The processes started, with their arguments, are the same on both sides at every step.

Why: the folder was a path nothing had checked, and stayed one that is not a folder.

Code: `validateDirProc` in `qml/services/Wallpapers.qml`.

Tests: `crates/qml/tests/files_equiv.rs` (`wallpaper_processes_match_the_original`: 24 steps, the
processes equal at each; the folder equal at 20 and, at the 4 after a dangling link and a
missing path, the original's value and the one here are each stated) and
`crates/qml/tests/service_failures.rs` (`wallpaper_folder_is_set_only_to_a_checked_path`).

Not checked: the picker on screen.

## A formula MicroTeX fails on, and an empty formula

`LatexRenderer.requestRender(expression)` starts MicroTeX once per formula and answers
`[hash, isNew]`; `renderFinished(hash, imagePath)` says an image is there.

| | the original | Here |
|---|---|---|
| MicroTeX exits with an error | The formula stays in `processedHashes` and `processedExpressions`, its path goes to `renderedImagePaths` and `renderFinished` is sent, for an image that was not made. | A warning is logged; the formula is taken out of `processedHashes` and `processedExpressions`; no path, no `renderFinished`. |
| The same formula is asked for again | `[hash, false]`, MicroTeX is not started, `renderFinished` is sent again for the missing image. | `[hash, true]`, MicroTeX is started again. |
| The empty formula | MicroTeX is started (it exits 1 with no image) and the row above applies. | `[hash, false]`; MicroTeX is not started, nothing is kept and nothing is sent. |
| A chat message whose formula has no image | The formula is replaced by a link to the missing image. | The formula stays as written. |
| MicroTeX exits 0 | Kept, path stored, `renderFinished` sent. | The same. |

Why: a formula that failed once was never tried again, and was reported with an image that
does not exist. MicroTeX itself exits 1 on the empty formula, so starting it gives nothing.

Code: `requestRender` and `onExited` in `qml/services/LatexRenderer.qml`;
`handleRenderedLatex` in `qml/modules/rusty/sidebarLeft/aiChat/MessageTextBlock.qml`.

Tests: `crates/qml/tests/files_equiv.rs` (`latex_failed_and_empty_renders_differ_from_the_original` with
a stand-in for MicroTeX and `latex_real_microtex_failures_differ_from_the_original` with MicroTeX itself:
each step with the original's answer and the one here; `latex_render_matches_the_original` and
`latex_render_matches_real_microtex` hold everything else equal to the original) and
`crates/qml/tests/service_failures.rs` (`failed_latex_render_is_logged_and_tried_again`,
`empty_latex_formula_is_not_rendered`, `chat_message_keeps_a_formula_without_an_image`).

Not checked: the chat on screen; the original's chat message with a failed formula is read from its
code, not run.

## The mode tabs of the region selector do not write the mode before they are given one

`qml/modules/rusty/regionSelector/OptionsToolbar.qml` binds the tab bar's `currentIndex` to
`selectionMode` and writes `selectionMode` back in `onCurrentIndexChanged`. `selectionMode`
has no value of its own: a `Synchronizer` in `RegionSelection.qml` gives it one after the
toolbar is created.

| | the original | Here |
|---|---|---|
| The handler | `root.selectionMode = currentIndex === 0 ? RectCorners : Circle` | The same line after `if (root.selectionMode === undefined) return;` |
| While the toolbar is created | `selectionMode` is `undefined`, so the binding gives `currentIndex` 1; the handler writes the mode inside that evaluation, and the binding is asked to run again while it runs. | The handler does nothing; the `Synchronizer` then gives the mode and the binding gives the index. |
| What Qt 6.12 prints | `QML TabBar: Binding loop detected for property "currentIndex":` and the place of the binding. | Nothing. |
| The mode and the index after loading, after a write of the mode, after a press on a tab | The same in both. | The same in both. |

Why: with the original's handler this engine reports the same binding loop Qt 6.12 does, and
`riaotcheck --strict` (the `aot` step) fails on any script error. Allowing that one error in
the check would hide it; the handler has nothing to write before there is a mode.

Code: `onCurrentIndexChanged` of the `ToolbarTabBar` in
`qml/modules/rusty/regionSelector/OptionsToolbar.qml`.

Tests: `crates/qml/tests/audit_r4.rs`
(`guarded_mode_tabs_differ_from_the_original_only_by_the_binding_loop`), over
`crates/qml/tests/audit_r4/mode_tabs_original` (the original's handler) and `mode_tabs_guarded` (this
one), each the toolbar reduced to its tabs under the `Synchronizer`. In Qt 6.12 and in this
engine both print the same six lines of `selectionMode` and `currentIndex` (loaded, the mode
written twice, each tab pressed, the mode written again); the original's handler adds the two lines of
the binding loop in Qt and one error here.

the original's own file under Quickshell 0.3.2 (the guest of `bench/vm.sh`): each time the region selector
opens, the log has `QML TabBar at @modules/common/widgets/ToolbarTabBar.qml[92:5]: Binding loop
detected for property "currentIndex":` with `OptionsToolbar.qml:32:9`. This shell logs nothing
there, and a press on a tab selects it in both.

## The wallpaper search of a background widget is given the new values by the handler

`qml/modules/rusty/background/widgets/AbstractBackgroundWidget.qml` starts the search for a place
on the wallpaper from `on…Changed` handlers, and the search reads bindings that depend on the
property that changed. Qt 6.12 runs a property's own handler before the bindings that read
the property, so in the original the search starts with the values from before the change: it is a
Quickshell `Process`, whose `command` is such a binding, and `running = true` starts it at
once with the command it has (`Process::setRunning`).

| Handler | the original under Qt 6.12 | On screen in the original | Here |
|---|---|---|---|
| `onPlacementStrategyChanged` | The search runs with the previous strategy: `mostBusy` chosen, `busiest false`. | The clock and the weather widget stay where the previous strategy put them until the wallpaper changes. | `refreshPlacementIfNeeded()` writes `busiest` itself, as it already wrote the image: the widget moves when the strategy is chosen. |
| `onScaledScreenWidthChanged`, `onScaledScreenHeightChanged` | the original has no such handlers; they are this tree's (a widget made before its screen is measured). | | The same function writes `screenWidth` and `screenHeight`. Without that the search of such a widget ran for a screen 0 high. |

While a document is created the original gets the right values: the `Process` has no command yet and
starts when the binding gives it one. The difference is in changes made later.

Seen in the guest of `bench/vm.sh`, the original under Quickshell 0.3.2 and this shell on the same
wallpaper (flat on the left, a busy patch on the right), `placementStrategy` of the clock
changed from `leastBusy` to `mostBusy` in the config: in the original the clock stays on the flat part
and moves to the patch once the wallpaper changes; here it moves at once.

Why: the middle columns are a fault of the original's file, not something a user of the original relies on. This
engine now runs handlers in Qt's order, so with the original's file it would do the same.

Test: `crates/qml/tests/handler_values.rs` over `crates/qml/tests/handler_values/placement`
runs the file of this tree among stand-ins for what it imports, in Qt 6.12 and in this
engine; the `// qt: ` lines of the case's `main.qml` are what both print. With the handlers
as they were, Qt 6.12 and this engine both print the previous values.

## The agent's name over an answer of the assistant chat

In the tree `qml` began as the chat ran one agent, and every answer was headed with that
agent's fixed name and the model. Here the agent is the command set in `sidebar.assistant.command`, which can be changed
while the chat has answers in it.

| | That tree | Here |
|---|---|---|
| The header of an answer | "`<fixed name>` · `<model>`", the same for all. | The name the agent that wrote it gave itself, and the model. An answer keeps it when another agent is put in the config. |
| A name wider than the sidebar | Could not happen: the names were fixed and short. | An agent that gives no name is called by its program. In the empty chat's title and in the hint of the input field such a name is on as many lines as it takes, not cut. |

Why: after the agent is changed the old answers are still in the chat, and the new agent's name
over them would say it wrote them.

Code: `agent` in `qml/services/assistant/AcpMessageData.qml`, set by `addMessage` in
`qml/services/assistant/AcpSession.qml`; the header in
`qml/modules/rusty/sidebarLeft/assistant/AssistantMessage.qml`; `inputHint` in
`qml/modules/rusty/sidebarLeft/AssistantChat.qml`; the title in
`qml/modules/common/widgets/PagePlaceholder.qml`.

Tests: `crates/shell/tests/assistant_session.rs`
(`answer_keeps_the_name_of_the_agent_that_wrote_it`) and
`crates/shell/tests/assistant_agent_name.rs`.

## No image search on booru sites

the original's left sidebar has an "Anime" tab, shown when `policies.weeb` is not 0, that searches booru
sites for images. The shell has no such search.

| | the original | That tree | Here |
|---|---|---|---|
| The page | `SidebarLeftContent.qml` adds the tab and creates the page under `policies.weeb`. | Declared (`Component { id: anime; Anime {} }`) and never created: the tab list and `contentChildren` hold the assistant and the translator only, whatever `policies.weeb` is. | No declaration. |
| Its files | `Anime.qml`, `anime/`, `services/Booru.qml`, `services/BooruResponseData.qml`. | The same files, which only the page that is never created names. | Not in `qml`. |
| Its options | `sidebar.booru`, `Persistent.states.booru`. | Declared, read by those files only. | Gone. |

Why: `qml` does what the tree it began as did, and no user of that tree could open the page.
Files and options that nothing reaches are not shipped.

One thing that tree did at every start is gone with them: `Directories.qml` emptied and
recreated the previews directory `media/boorus` under the cache, which nothing then wrote to.

`policies.weeb` still does the rest of what it did: the Konachan choice among the random
wallpapers (`WallpaperSelectorContent.qml`, `QuickConfig.qml`, `welcome.qml`) and its switch in
the settings.

Tests: none. There is no page to open; that the tab list and `contentChildren` name no such
page was read in both trees, not run.

## A long press on a `GroupButton`

the original's `onPressAndHold` ends with `root.clicked = false`, a write to the button's `clicked`
signal. Qt 6.12 refuses it, so the original logs `TypeError: Cannot assign to read-only property
"clicked"` on every long press and nothing else comes of the line. Here the line is gone: a
long press does the same and logs nothing.

Code: `onPressAndHold` in `qml/modules/common/widgets/GroupButton.qml`.

Test: `crates/shell/tests/interact.rs` (`controls_match_qt`, the `grp` steps of
`tests/interact/steps`): the same long press and click under Quickshell on Qt and here, and no
refused write in either log.

## The colour picker after a panel Hyprland animates away

The original closes the panel and starts hyprpicker from a timer: 300 ms after the right sidebar, `sleep 0.2`
after the region selector. Here hyprpicker starts on the first frame shown after Hyprland's own
close animation of the panel's layer: `closelayer` of the layer on Hyprland's event socket, then
the longer of `layersOut` and `fadeLayersOut` from Hyprland's settings, with nothing added. The
panel leaves the screen the same way in both.

Measured on Hyprland 0.57.1, 20 pickers a row: screenshots taken as hyprpicker starts that have
the panel on them, and the time from the call to hyprpicker's start (median).

| | the original | Here |
|---|---|---|
| The dots' config, right sidebar (240/270 ms) | 0, 343 ms | 0, 315 ms |
| Default animations (800 ms) | 20, 231 ms | 0, 857 ms |
| `fadeLayersOut` 500 ms (`layersOut` 800 ms) | 16, 238 ms | 0, 852 ms |
| The dots' config, region selector (layer rule `no_anim`) | 0, 230 ms | 0, 322 ms |

Three limits of it:

- A layer with the rule `no_anim` waits the general length. Hyprland 0.57.1 gives a client no
  way to read a layer's rules (no `hyprctl` command, not in `hyprctl -j layers`, not in its Lua
  objects), and the shell does not read Hyprland's config or guess by the layer's name. The panel
  is gone at once, as Hyprland shows it, and hyprpicker starts at 322 ms where the original's starts at
  230 ms.
- Animation settings changed by `hyprctl keyword` or `hyprctl eval` are not seen until the
  config is reloaded: the shell reads them at its start and on `configreloaded`, and Hyprland
  sends none for those. Until then the wait is the length read before.
- The popups of the panel get an empty last frame and disappear at once: Hyprland fades them on
  their own (`fadePopupsOut`) and says nothing of it. What the original's popups do there was not measured.

Without Hyprland's event socket or an answer to the settings, with a spring curve (it has no
fixed length), and on another compositor the panel gets an empty last frame too and hyprpicker
starts on the next frame; on Hyprland a line in the log says why.

Code: `crates/shell/src/hyprclose.rs`; `keep_last_frame`, `hypr_closed`, `run_closed` and
`blank_closing` in `crates/shell/src/main.rs`.

Tests: `crates/shell/tests/color_picker_off_screen.rs`
(`hyprpicker_starts_after_hyprlands_close_animation`,
`hyprpicker_does_not_wait_with_hyprlands_animations_off`,
`a_reload_of_hyprlands_config_reads_its_animations_again`,
`hyprpicker_starts_for_a_layer_hyprland_does_not_say_it_closed`,
`hyprpicker_starts_when_hyprlands_event_socket_is_lost`,
`a_closed_panel_gives_an_empty_frame_to_a_hyprland_that_does_not_answer`) and the unit tests of
`hyprclose`.

Not checked in the gate: a real Hyprland (the numbers above are from a guest with 0.57.1), and
the three limits, which no test holds.

## The visualizer of the media controls rests while nothing can be seen

The original runs `cava` for as long as the media controls are open, also in a locked session and with
every output powered off, and takes 60 lines of bars a second from it there. Here the audio
capture behind `CavaVisualizer` stops when the compositor confirms the session lock or powers
the last output off, and starts again with the unlock or the first output that is on. The
visualizer's `running` stays true and its `points` keep the last frame meanwhile, so what is
drawn on a screen that is on is the same.

The wave of the player's progress bar (a `FrameAnimation` of `StyledProgressBar.qml`) rests in
the same two states, where the original repaints it at every frame:
[quickshell-differences.md](quickshell-differences.md), "A `FrameAnimation` behind a lock or on
a dark screen".

Code: `set_unseen` in `crates/qml/src/cava.rs`, `sync_unseen` in `crates/shell/src/main.rs`.

Tests: `crates/shell/tests/visualizer_off_screen.rs`; `nothing_on_screen_stops_the_capture` in
`crates/qml/src/cava.rs`.

## The rusty-impulse card of the About page has no Issues and Discussions buttons

The settings page "About" of the tree `qml` began as (`ref/ii`, the tree Quickshell runs in the
screenshot comparison of `tools/parity`) has four buttons under its rusty-impulse card:
Docs, Issues, Discussions and Donate. Here the card has Docs and Donate.

| | That tree | Here |
|---|---|---|
| Buttons of the card | Docs, Issues (`https://github.com/end-4/dots-hyprland/issues`), Discussions (`https://github.com/end-4/dots-hyprland/discussions`), Donate. | Docs and Donate, with the addresses they had. |
| "Report issue" of the card above | Opens `repoUrl` + `/issues`. | The same. |

Why: the project's own links open rusty-impulse (#53), and the card for reports about this
shell is the one above. The wiki and the sponsor page are still end-4's.

Code: the `Dotfiles` section of `qml/modules/settings/About.qml`.

Tests: none. In the screenshot comparison with that tree under Quickshell the row of buttons
differs: the two buttons are missing and Donate stands where Issues is there.

## "Keep awake" after the monitor of its window is removed

"Keep awake" (`Idle.inhibit`) holds its idle inhibitor on a window of its own, a `PanelWindow`
for no screen in particular. The compositor puts that window on a monitor and closes it when
that monitor is removed, and a window the compositor closed stays closed (Quickshell's
`ProxyWindowBase::onVisibleChanged` sets `visible` to false; so does rusty-impulse). A monitor that
leaves the bus while it is switched off is removed and added again each time.

| | the original | Here |
|---|---|---|
| The monitor the window is on is removed, others remain | The window stays closed. | The window is opened again, on a monitor the compositor picks, and the inhibitor is made again on it. |
| The only monitor is removed and one is added | The window stays closed. | The window is opened again once there is a monitor. |
| "Keep awake" was on | The toggle stays on and the state file keeps `idle.inhibit: true`, but there is no inhibitor: the session goes idle until the shell restarts. | The session is kept from going idle, as before the monitor was removed. |
| "Keep awake" was off, turned on after the monitor is back | The toggle turns on and nothing is inhibited. | The session is kept from going idle. |
| "Keep awake" stays off | The session goes idle. | The same. |
| Layers the compositor lists (one 1920x1080 monitor) | `quickshell 1x1+1919+1079` is gone until the shell restarts. | It is back. |

Seen in the guest of `bench/vm.sh` (Hyprland 0.57.1, the one monitor unplugged for 5 s and
plugged back, idleness watched with `swayidle`): the original under Quickshell 0.3.2 and the original's file under
rusty-impulse lose the window for good, and the original's file under rusty-impulse then goes idle with the toggle on;
with this file the session stays awake. Hyprland moves the layers to a `FALLBACK` monitor in
between, so the window is opened twice, once per monitor removed. Switching the screen off
without removing the monitor (`dpms off`) closes nothing.

Why: a toggle that shows on while the session goes idle is a fault for the user, whichever
shell had it first. The change is in the file, not in rusty-impulse: a closed window still stays
closed, as in Quickshell, and the file asks for it again.

Code: `reopenWindow` in `qml/services/Idle.qml`, called when the window is closed and when
the number of screens changes.

Tests: `keep_awake_holds_after_the_only_monitor_is_unplugged_and_another_plugged_in` and
`keep_awake_holds_after_its_windows_monitor_is_unplugged` in
`crates/shell/tests/windows_outputs.rs`, each on a sway of its own. With the original's file both fail:
the closed window is not opened again.

Not checked: the file under Quickshell.

## "Auto, " of the Night Light tile is translated in Russian

The status of the Night Light tile is `Translation.tr("Auto, ")` followed by the translation of
"Active" or "Inactive". the original's `ru_RU.json` has the key without its space, `"Auto,"`, so the original finds no
translation and shows the English text.

| | the original | Here |
|---|---|---|
| The key in `ru_RU.json` | `"Auto,": "Авто,"`, which no `Translation.tr()` call names. | `"Auto, ": "Авто, "`, the text the tile asks for (#65). |
| The tile in Russian, night light automatic and off | "Auto, Неактивно" | "Авто, Неактивно" |

The German, Spanish, French, Portuguese and Turkish files still have the original's key without the space
and show "Auto, " in English, as the original does. The Chinese file has the key with the space in both
trees.

Code: `statusText` in `qml/modules/common/models/quickToggles/NightLightToggle.qml`;
`qml/translations/ru_RU.json`.

Tests: none. In the screenshot comparison of `tools/parity` with the interface in Russian the
tile's second line differs.

## The unused quick toggles in edit mode are the toggles of their names

In edit mode (the pencil of the right sidebar) the panel lists the toggles that are not in use
under the ones that are. the original's `AndroidQuickPanel.qml` makes that list anew, of new objects,
each time the toggles in use change. Quickshell's ScriptModel then writes
new rows over the ones it has, in place, and a `DelegateChooser` does not choose
again for a row whose data changed: a button keeps the name, icon and look of the toggle it was
made for and holds the type of another.

| | the original | Here |
|---|---|---|
| The unused list | New `{ type, size }` objects at every change. | One object for each type, made once (`unusedEntries`); the unused list is those not in use. |
| The unused list after a click on the unused Dark Mode, with the default toggles | Dark Mode's button is still there and holds Cloudflare WARP; each button after it in its row holds the type of the toggle next to it; no button looks like Color picker, which is now the last of that row. | Dark Mode's button is gone, every other one is the toggle it was. |
| A click on the button that shows Cloudflare WARP then | Adds Game mode. | Adds Cloudflare WARP. |

Why: a click on a toggle has to add the toggle it shows.

The toggles in use show the same fault under Quickshell once one of them is removed. Here they
do not, with the original's own QML: [qt-differences.md](qt-differences.md), "Models of delegates", the
element of a `list<var>`.

Code: `unusedEntries` and `unusedToggles` in
`qml/modules/rusty/sidebarRight/quickToggles/AndroidQuickPanel.qml`.

Tests: `crates/shell/tests/quick_toggle_edit.rs`: after each click, right click and wheel notch
in edit mode every button is the component of the type it holds. In the screenshot comparison
of `tools/parity` the scene `ptr_quickToggleEdit` differs from that tree under Quickshell from
its first added toggle on.

## The right sidebar opens as it first did

The original keeps the content of the right sidebar loaded while the sidebar is closed
(`sidebar.keepRightSidebarLoaded`, on by default), and what was left in it is there at the next
opening: the calendar on the month the wheel paged it to, the list scrolled. Of all that the original
clears four flags as the sidebar closes, those of the Wi-Fi, Bluetooth and the two audio
dialogs, and the dialog that was open stays on the screen all the same: its Loader, once it has
shown the dialog, is no longer bound to the flag and unloads only after the dialog's own
`dismiss`. Here the sidebar opens as it did the first time.

| At the opening after | the original | Here |
|---|---|---|
| The wheel, Page Up or Down or the arrows over the calendar | The month it was left on, with the dot before its name. | The current month. |
| The pencil (edit mode of the quick toggles) | Still on. | Off. |
| The detail dialog of Wi-Fi, Bluetooth, Night Light, audio output or input | Still open. | Closed. |
| A scroll of the notifications, of a list of tasks or of the stopwatch's laps | Where it was left. | At the first row. |
| An expanded notification group | Expanded. | Folded. |
| The tab "Done" of the to-do list, the tab "Stopwatch" of the timer | That tab. | The first tab: "Unfinished", "Pomodoro". |
| The "Add task" dialog of the to-do list | Open, with the text typed so far. | Closed, and the text is gone. |
| The tab of the calendar, to-do and timer block, and whether the block is folded | As left: both are saved in `states.json` and outlive the shell. | The same. |

Why: asked for by the owner. A calendar found on a month of next year, with nothing to say why
but the dot before its name, reads as a wrong date.

What is put back is the state the shell forgets when it ends. The choices the original writes to
`states.json` stay, and so does everything that is not the look of the sidebar: a running timer,
the laps, the tasks, the notifications. The left sidebar is as in the original.

The state is put back twice, by one handler of `GlobalStates.sidebarRightOpen`: as the sidebar
closes, so that the opening shows nothing moving back, and as it opens. The lists go to their
first row at once, not by their scroll animation (`scrollToStart` of `StyledListView.qml`). A
dialog whose flag is cleared closes as on `dismiss`, by its own animation, and is unloaded
(`ToggleDialog` of `SidebarRightContent.qml`).

Code: the `Connections` on `GlobalStates` in `SidebarRightContent.qml`,
`calendar/CalendarWidget.qml`, `notifications/NotificationList.qml`, `todo/TodoWidget.qml`,
`todo/TaskList.qml`, `pomodoro/PomodoroWidget.qml` and `pomodoro/Stopwatch.qml` of
`qml/modules/rusty/sidebarRight`; `collapseGroups` in
`qml/modules/common/widgets/NotificationListView.qml`.

Tests: `crates/shell/tests/sidebar_right_reset.rs`, with the original's own blocks and with the widgets of
a layout's sidebar.

## The quick toggles move to their places in edit mode

In edit mode (the pencil of the right sidebar) the original shows each change at once. The unused
toggles are there, whole, at the frame after the click on the pencil. A toggle that is added,
removed, resized or moved by the wheel changes rows of the panel, and a button whose row
changed is destroyed and made anew: it and the buttons after it are drawn in their new places
at the next frame. Here the same changes are shown as motion; where the buttons are when it is
over, and how they look, is as in the original.

| | the original | Here |
|---|---|---|
| A click on the pencil | The panel is as high as with the unused toggles, and they are at full opacity, one frame later: making them takes longer than their fade, and than the fast start of the panel's growth. | The unused toggles are made first; the panel grows and they fade in from the frame after. |
| A toggle is added, removed or moved by the wheel | Every button the change puts elsewhere is in its new place at the next frame. | Each of them slides there from where it was drawn, a button made anew in another row too. The line over the unused toggles slides with them. |
| A right click on a toggle (its size) | The button's width grows or shrinks over time, as in the original; the buttons pushed to another row jump there, the icon jumps between the middle and the left edge, the text is there at once. | The pushed buttons slide; one made anew starts as wide as it was. The icon slides, the text fades in. |

Why: asked for by the owner.

A button writes where it is drawn before the toggles change (`movedFrom` of the panel, by
type), and starts from there once the layout has placed it: a `Translate` that goes to nothing
by `Appearance.animation.elementMove`. The engine has no transitions of positioners to do it.
The button starts these animations itself: a `Behavior` of a button made anew does not animate
before the button's first layout.

Code: `unusedShown`, `movedFrom` and `beforeMove` in
`qml/modules/rusty/sidebarRight/quickToggles/AndroidQuickPanel.qml`; `slide`, `widthSlide` and
`contentSlide` in `androidStyle/AndroidQuickToggleButton.qml`.

Tests: `motion_interp` and `motion_compiled` of `crates/shell/tests/quick_toggle_edit.rs`: after
a click on the pencil the panel grows over many frames, from the frame after the one that made
the unused toggles; after a click and a right click every button that ends elsewhere, or of
another width, is drawn between the two for many frames and never goes more than half of the
way in one.
