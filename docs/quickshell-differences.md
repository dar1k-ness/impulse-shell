# Deliberate differences from Quickshell

impulse-shell does what Quickshell does with the same QML. The cases below are the ones where it
does something else on purpose. Each row names the test that holds the difference: if
Quickshell changes, or impulse-shell stops doing what the row says, that test fails.

## A reload while the session is locked

Compared with Quickshell 0.3.2 under sway, on the original's lock screen (`qml/modules/rusty/lock/Lock.qml`).

| | Quickshell 0.3.2 | impulse-shell |
|---|---|---|
| `Quickshell.reload()` under a held lock | Loads the next generation at once and hands it the held lock object (`WlSessionLock::onReload`). the original's `GlobalStates.screenLocked` starts `false` in every generation, so the new `WlSessionLock` sends `unlock_and_destroy`: the session is unlocked, the desktop shows. | Holds the reload. The same image keeps the lock and its surfaces. Once QML unlocks and the compositor has answered the `wl_display.sync` sent after `unlock_and_destroy`, the shell re-executes. |
| The typed password | Gone with the old generation. | Still in the field while the reload waits. |
| A PAM conversation in progress | Aborted (`PamContext` is not handed over). | Goes on, and takes its answer. |

Why: in `ext-session-lock-v1` only the lock client can unlock the session. In Quickshell with
the original's lock screen the unlock is a side effect of the reload, not something the user did.
Doing the same would let a configuration reload open a locked session.

Tests, in `crates/shell/tests/session_lock_reload.rs`:

- `a_reload_under_the_original_s_lock_screen_unlocks_in_quickshell_and_waits_in_impulse_shell` runs both shells
  on the same `Lock.qml` and checks both columns of the first row.
- `the_typed_password_and_a_pam_conversation_outlive_a_held_reload` checks the other two rows
  for impulse-shell.
- `reload_under_a_session_lock_waits_for_the_unlock` and
  `unlock_and_reload_in_one_turn_unlocks_first` check the order of unlock, sync and re-execution.

Two more paths of Quickshell 0.3.2 have no counterpart in impulse-shell. Both end `qs` on a Wayland
protocol error and leave the session locked with no lock surface. They were seen in a run
under sway and are not held by a test:

- a hard reload (`Quickshell.reload(true)`) under a held lock: the lock object is destroyed
  while locked ("the session lock may not be destroyed while locked");
- a `locked` value kept across generations: the new generation asks for a lock surface on an
  output that still has the old one ("session lock surface already created for the given
  output").

impulse-shell has one kind of reload, and it waits for the unlock in both cases.

Not checked: Hyprland. All of the above was run under sway.

## An icon no theme has

Compared with Quickshell 0.3.2 under sway, on an `Image` whose source is `image://icon/nope`.

| | Quickshell 0.3.2 | impulse-shell |
|---|---|---|
| What is drawn | A black and `#d900d8` checkerboard (`IconImageProvider::missingPixmap`). | The theme's `application-x-executable`, else its `image-missing`, from the directory an icon of that request would have come from; nothing when the theme has neither. |
| The image's size | The request, each side rounded up to even. | The same. |

Why: the original takes icon names from data at 14 places in `qml/`. Four have no fallback name: a
tray item's icon (`modules/rusty/bar/SysTrayItem.qml:74`), a search action's
(`modules/rusty/overview/SearchItem.qml:289`), the distribution's logo
(`modules/settings/About.qml:135`) and a user directory's
(`modules/common/widgets/DirectoryIcon.qml:22`). The other ten fall back to `image-missing`,
which a theme may lack. A program whose icon the theme does not have would stand in the bar
as a purple and black square.

Tests:

- `missing_icon_is_the_generic_one_a_found_icon_would_be` in
  `crates/shell/tests/icon_request_scale_equiv.rs` checks the first row in both shells.
- The other tests of that file compare the size of the `missing` Image in both shells.

## A shell that ends right after it unlocked

Compared with Quickshell 0.3.2 under sway.

| | Quickshell 0.3.2 | impulse-shell |
|---|---|---|
| `locked = false` and `Qt.quit()` in one handler | Sends `unlock_and_destroy` and ends. It does not wait for the compositor. | Sends `unlock_and_destroy` and a `wl_display.sync`, and ends once the compositor has answered the sync or the connection is gone. |
| SIGTERM after `locked = false`, before the compositor has read the unlock | Dies by the signal without waiting: 41 ms after the SIGTERM in one run, with `unlock_and_destroy` written and not read by the compositor. | The same wait, then the shell dies by the signal. |

Why: `ext-session-lock-v1` says of `unlock_and_destroy` that "a correct client that wishes
to exit directly after unlocking the session must use the wl_display.sync request to ensure
the server receives and processes the unlock_and_destroy request". A compositor that finds
the client gone before it has read the unlock keeps the session locked, with nothing left to
unlock it. This was seen under sway with an impulse-shell that a test killed right after its
`unlock_and_destroy`: sway never read the request, and the next client's panel got its
`configure` and never entered an output.

While it waits the shell runs nothing else, and a second SIGTERM does not end it: only the
compositor's answer, the end of the connection or SIGKILL do.

`$XDG_RUNTIME_DIR/ri-native.locked` (impulse-shell only) names the shell that holds the lock. A shell
that ends without the lock takes it away, also when the connection broke before the
compositor's answer. A shell that ends holding the lock leaves it: the session is locked with
a dead lock screen. The next shell to start removes a file that names a process that is gone.

Tests, in `crates/shell/tests/windows_outputs.rs`; all but the last hold back what the shell
writes to the compositor (`testenv::HeldRequests`):

- `quickshell_ends_before_the_compositor_has_read_its_unlock` runs Quickshell 0.3.2 through
  the first row, `quickshell_ends_on_sigterm_before_the_compositor_has_read_its_unlock`
  through the second.
- `quit_while_unlocking_waits_for_the_compositor` and
  `sigterm_while_unlocking_waits_for_the_compositor`: impulse-shell is alive until the compositor
  reads the unlock, also after a second SIGTERM; then the compositor shows a panel again and
  the shell has ended.
- `connection_lost_while_the_exit_waits_ends_the_shell`: the wait ends with the connection.
- `connection_lost_while_unlocking_leaves_no_marker` and
  `marker_of_a_shell_that_is_gone_goes_at_the_next_start`: the file.

Under Hyprland 0.57.1 in the benchmark guest (`bench/vm.sh`), `swaymsg` replaced by a script
that says the same to `hyprctl`, 20 runs of each test:

| Tests | `misc:allow_session_lock_restore = true` | The default, `false` |
|---|---|---|
| `lock_is_released_once_the_compositor_has_unlocked`, `sigterm_while_unlocking_waits_for_the_compositor`, `quit_while_unlocking_waits_for_the_compositor` | 20 of 20 each | 20 of 20 each |
| The two Quickshell tests | 20 of 20 each | 3 of 3 each |
| `connection_lost_while_the_exit_waits_ends_the_shell`, `connection_lost_while_unlocking_leaves_no_marker`, `marker_of_a_shell_that_is_gone_goes_at_the_next_start` | 20 of 20 each | 0 of 3 each |

The three of the last row end their shell with the session locked and then unlock it with a
second shell. Hyprland refuses that shell's lock (`finished`) unless the option is set
(`SessionLockManager.cpp`, "Cannot re-lock, misc:allow_session_lock_restore is disabled"),
so they stop there and leave the guest locked; what they check of the first shell is not
reached. sway takes the second lock.

## A session lock the compositor refuses

Compared with Quickshell 0.3.2 under sway and under Hyprland 0.57.1.

| | Quickshell 0.3.2 | impulse-shell |
|---|---|---|
| Lock surfaces of a new lock | Asked for in the same turn as the `lock` request (`WlSessionLock::realizeLockTarget`). | Asked for once the compositor has answered a `wl_display.sync` sent after `lock`: one round trip later. |
| The lock is refused (`finished` with no `locked`) under Hyprland | Destroys the lock and the surfaces it asked for. Hyprland has no object for those surfaces: `invalid object`, and `qs` exits. | `finished` comes before the answer to the sync, so no surface was asked for. Only `ext_session_lock_v1.destroy` is sent, and the shell stays up. |

Why: Hyprland refuses a lock of an already locked session while it handles the `lock` request
([`onNewSessionLock`](https://github.com/hyprwm/Hyprland/blob/v0.57.1/src/managers/SessionLockManager.cpp#L52-L59),
[`sendDenied`](https://github.com/hyprwm/Hyprland/blob/v0.57.1/src/protocols/SessionLock.cpp#L164-L167)),
and for a refused lock `get_lock_surface` creates no object
([`setGetLockSurface`](https://github.com/hyprwm/Hyprland/blob/v0.57.1/src/protocols/SessionLock.cpp#L121-L128)).
The protocol tells the client to destroy its lock surfaces, and that request on an id the
compositor never made ends the connection. A session is left locked this way when its lock
client dies. Waiting for the answer keeps to the protocol on every compositor: a lock refused
at once has no surface, and one ended later has surfaces the compositor did create.

The wait costs the lock screen one round trip and no frame. Quickshell runs Qt's threaded
render loop, in which every window draws in a thread of its own (seen in the Hyprland guest:
`qt.scenegraph.general` logs `threaded render loop`, and `qs` has one `QSGRenderThread` per
window), so a lock surface never waits for the frame of another window. impulse-shell draws all of
its windows in one loop, where a frame of another window (a blurred wallpaper takes long
without a GPU) would stand before the lock screen's. So the order of the frames is ours:
from the `lock` request on, the lock surfaces of a lock the compositor has not confirmed draw
first, and the other windows draw once each lock surface has committed its first frame. That
holds for the lock surface of an output connected before `locked` too. The other windows do
not wait for `locked`, which a compositor may send late or never. What is seen is the same in
both: the lock screen as soon as it can be drawn, and no window stops drawing.

The same order holds for the lock screen's keyboard. The compositor gives a lock surface the
keyboard focus when its first frame maps it, and in both shells the surface gets keys only from
the answer to the `wl_display.sync` sent at that `wl_keyboard.enter` on (QtWayland's
`handleWaylandSync`); a key handled before the answer reaches no item
(`crates/shell/tests/key_before_activation_equiv.rs`). impulse-shell reads no event while it draws, so
the other windows also wait for these two answers (the lock surface is mapped, its keyboard
focus is active): two round trips. Without that wait the frame of another window drawn right
after the lock screen's first one kept the lock screen without the keyboard for as long as
that frame took. Seen in a Hyprland 0.57.1 guest without a GPU, where the blurred wallpaper's
frame took 1.1 to 1.7 s: a password typed 1.5 s after the lock was lost in 5 of 90 rounds, and
its Return, handled after the answer, sent an empty one to PAM. A frame drawn after the answers
only delays the keys.

Measured in that guest, 30 locks each, the monitor turned off and on before each lock and the
password typed 0.3 to 1.5 s after it: the lock screen's keyboard focus was active 0.90 to
2.64 s after the `lock` request without the wait (median 1.30 s; the password was lost in 19
locks) and 0.05 to 0.16 s after it with the wait (median 0.09 s; lost in none). Quickshell
0.3.2 draws each window in a thread of its own; the keyboard focus of its lock surface was
active 0.12 to 2.46 s after the request (median 0.69 s), and the password was lost in 6 locks.

What QML reads after a refused lock is the same in both: `locked` false, `secure` false and
`secureChanged` emitted once.

Tests, in `crates/shell/tests/session_lock_finished.rs`:

- `a_lock_refused_as_hyprland_does_asks_for_no_surface` runs impulse-shell against a compositor that
  refuses as Hyprland does and checks the second row and what QML reads.
- `a_lock_after_a_refused_one_is_taken` checks that the next lock is asked for and held.
- `finished_before_locked_destroys` is the refusal by sway itself.
- `the_lock_screen_draws_before_any_other_window` checks that a panel that changes in the
  turn of the `lock` request commits its frame only after the lock surface's first frame,
  against a compositor that sends `locked` after that frame, as Hyprland does.
- `other_windows_draw_once_the_lock_screen_has_its_frame` checks that the panel then draws
  without `locked`, against a compositor that keeps `locked` back.
- `a_lock_surface_for_an_output_connected_before_locked_draws_first` checks both for an
  output connected while `locked` is kept back: its lock surface draws before the panels, and
  they draw after it.
- `no_window_draws_between_the_lock_screens_first_frame_and_its_keyboard` checks that a panel
  that changes in the turn of the lock draws only after the lock surface's keyboard focus is
  active, with `locked` kept back.

Not held by a test: the Quickshell column (seen in a Hyprland 0.57.1 guest and, for what QML
reads, under sway).

## A panel whose output is removed

Compared with Quickshell 0.3.2 under sway.

| | Quickshell 0.3.2 | impulse-shell |
|---|---|---|
| The compositor sends `zwlr_layer_surface_v1.closed` | The window is closed (`LayerSurface::zwlr_layer_surface_v1_closed`). | The same (`layer_closed`). |
| The panel's `wl_output` is removed and no `closed` comes | The window stays: `ProxyWindowBase::onScreenDestroyed` only forgets the screen. The panel is shown on the output the compositor picked. | The panel is closed with its popups and emits `closed` (`output_destroyed`). |

Why: the second row happens when the output is removed in the same milliseconds the panel is
created. The compositor reads `get_layer_surface` after the output is gone. The request names
a dead `wl_output`, so sway picks another output itself and never sends `closed`: the panel
of the unplugged screen stays on a live one for good. In every other order of the same events
the compositor closes the panel, so impulse-shell closes it in this one too.

Tests, in `crates/shell/tests/windows_outputs.rs`. The first two hold the compositor's
reading of the shell's requests until the output is removed:

- `quickshell_keeps_a_panel_created_while_its_screen_is_unplugged` checks the Quickshell
  column of the second row.
- `panel_created_while_its_screen_is_unplugged_is_closed` checks the impulse-shell column of the
  second row.
- `panel_for_unplugged_screen_is_not_moved_to_another_output` checks for impulse-shell that a panel
  already shown on the output is closed when the output is removed.

Not checked: Hyprland. The first row for Quickshell was seen in a run under sway and is not
held by a test.

## The report of a binding loop

Compared with Qt 6.12 (`qml6`) and Quickshell 0.3.2 on `crates/shell/tests/lazy_loader/loop.qml`
and `lazyl.qml`. Quickshell prints Qt's report through its own log.

Not meant to stay: the text, the place and the report on every occurrence are to be made Qt's
once the rework of the engine's binding table (R2) is merged, because the line and column of a
binding have to be kept where bindings are created (`Eng::apply`), which that rework rewrites.

| | Qt 6.12 | Quickshell 0.3.2 | impulse-shell |
|---|---|---|---|
| The text, for property `a` of the `Item` at 2:1 declared at 7:5 | `file:///…/loop.qml:2:1: QML Item: Binding loop detected for property "a":` and, on the next line, `file:///…/loop.qml:7:5` | `WARN scene: QML Item at @loop.qml[2:1]: Binding loop detected for property "a":` and `qs:@/qs/loop.qml:7:5` | `Binding loop detected for property "a" (Item in /…/shell.qml)`, target `ri::binding` (docs/env.md). The object's type and file; no line or column of the object or of the binding. |
| When | Every time the loop happens: 3 times in `loop.qml`. | The same, 3 times. | Once per binding in the life of the process. The engine also records the loop each time as an error; the shell prints an error once (`qml: binding loop detected on item N property a (…)`). |
| The property reported | `a` | `a` | `a` |
| The values of the bindings afterwards and how many times each was evaluated | | The same as Qt. | The same as Qt. |

Tests, in `crates/shell/tests/lazy_loader_equiv.rs`:

- `a_binding_loop_ends_as_in_qt` runs `qml6`, Quickshell and impulse-shell on `loop.qml`: the same 17
  console lines (values and evaluation counts after each of 3 changes) and the same property.
- `a_binding_that_read_item_is_evaluated_as_many_times_as_in_quickshell` does the same for
  bindings that read `item` of a loading LazyLoader, on `lazyl.qml`.

## A script that never returns

Compared with Quickshell 0.3.2.

| | Quickshell 0.3.2 | impulse-shell |
|---|---|---|
| A binding or handler that loops for ever (`while (true) {}`) | Stays in it: no limit. The shell stops answering; in `Component.onDestruction` at `Qt.quit()` the process never ends. | Interrupted with `Error: Interrupted` once it has used between 5 s and 10 s of its thread's CPU time. The shell goes on; at `Qt.quit()` the remaining handlers run and the process ends with its status. |
| A binding or handler that works for long and returns (1 s of CPU time, 8 MiB written with a `FileView`) | Runs to its end. | The same: only CPU time counts, and less than 5 s of it is never interrupted. |

Where the number is from: `DEFAULT_TIMEOUT` in `crates/qml/src/watchdog.rs`, 5 s, the time
after which a compositor takes a client for hung (mutter's `check-alive-timeout`). It is
between one and two intervals because the thread's timer is armed once per interval, not per
evaluation: an evaluation that starts while the timer runs for an earlier one gets a full
interval of its own when that timer expires.

Why: a shell that hangs in one script takes the bar, the lock screen and the session's exit
with it.

Tests, in `crates/shell/tests/destruction_equiv.rs` unless said:

- `a_handler_that_never_returns_is_interrupted` checks the impulse-shell column for
  `Component.onDestruction` at `Qt.exit(3)`: the handlers after the interrupted one, the file
  one of them writes and the status are those of the same configuration with a handler that
  returns. `Interrupted` is printed as a warning inside the interrupted handler, before the
  handlers after it run. Quickshell has no line to compare this with: its handler never
  returns.
- `the_error_of_a_handler_is_printed_before_the_next_handler_runs` compares that place with
  Quickshell on what can be compared: a handler that throws `new Error("Interrupted")`
  itself. Both print the error as a warning (Quickshell: `WARN scene: ...: Error:
  Interrupted`), not as a console line, between the lines of that handler and of the next.
- `a_handler_that_works_for_a_second_is_not_interrupted` checks the second row for both.
- `only_an_evaluation_that_alone_runs_out_of_time_is_interrupted` in
  `crates/qml/src/watchdog.rs` checks the limit itself.

Not held by a test: that Quickshell stays in the handler. Seen in one run: `qs` with the same
configuration was still in the handler 10 s after `Qt.quit()` and was killed.

## The exit while a Bluetooth write waits behind an unanswered one

Compared with Quickshell 0.3.2.

| | Quickshell 0.3.2 | impulse-shell |
|---|---|---|
| `Qt.quit()` right after a call or a property write to bluetoothd (`discovering = false` in `Component.onDestruction`) | Writes the call to the bus and ends without waiting for the answer. | The same: the exit waits until the call is written, not for the answer. A bluetoothd that does not answer does not hold the exit. |
| The same, while an earlier property write is still unanswered | Writes it at once. | Property writes go one at a time: the new one is written when the earlier one is answered. The exit waits for that up to 2 s (`FLUSH`, `crates/qml/src/notifier.rs`, one limit for every service the exit waits for), then ends without it. |

Measured, from `Qt.quit()` to the end of the process, three runs each, with the handler
stopping a discovery: Quickshell 18 to 31 ms, impulse-shell 18 to 42 ms, the same with a bluetoothd
that answers and with one that is stopped; without a discovery 17 to 25 ms for both.

Tests:

- `the_discovery_a_handler_stops_at_exit_is_stopped_on_the_bus` in
  `crates/shell/tests/destruction_equiv.rs` checks for both that `StopDiscovery` reaches
  bluetoothd.
- `bluetooth_flush_waits_for_the_write_to_be_sent_not_answered` in
  `crates/qml/tests/sysbus_services.rs` checks that an unanswered write does not hold the
  exit and that one queued behind it holds it for the limit.

- `the_exit_waits_once_for_two_services_that_do_not_answer` in
  `crates/shell/tests/destruction_equiv.rs` checks that the limit is one for all: with a
  write queued behind one bluetoothd holds and a notification for a session bus that never
  answers, impulse-shell ends 2 s after its handlers, not 4.

With the real bluetoothd 5.87, by hand in the guest of `bench/vm.sh` (a virtual controller:
`modprobe hci_vhci`, `btvirt -l2` built from the BlueZ source), one run: the Bluetooth page
of the Waffle action center open and `Discovering: yes`, then `impulse-shell -c impulse-shell kill`. The shell
ended in 64 ms with status 0, `StopDiscovery` is in `dbus-monitor`, `stop_discovery()` in
the log of `bluetoothd -d`, `Stop Discovery` in `btmon`, and `bluetoothctl show` says
`Discovering: no`.

Not held by a test: the times above, the run with the real bluetoothd, and that Quickshell
writes a second property write at once (read in its source, `src/dbus/properties.cpp`:
`asyncCall` per write).

## The compositor goes away

Compared with Quickshell 0.3.2, with a sway that is killed and one that is left by
`swaymsg exit`.

| | Quickshell 0.3.2 | impulse-shell |
|---|---|---|
| Warning, status | `The Wayland connection broke. Did the Wayland compositor die?`, 255 | The same |
| `Component.onDestruction` | Not run: what a handler would remove stays. | The same |
| The `Process`es of the configuration | Left running, with no parent. | Ended before the shell ends. |
| What is pending (`Config.qml`'s delayed save) | Lost. | Written out. |

Why: a program the shell started for itself (`cava`, a `sleep`) has no use once the session
is over, and nothing else would end it.

Tests:

- `no_handler_runs_when_the_compositor_goes_away` in
  `crates/shell/tests/destruction_equiv.rs` checks the first three rows for both.
- `lost_compositor_writes_the_pending_config_save` in
  `crates/shell/tests/wayland_lost_flush.rs` checks the impulse-shell column of the last.

Not held by a test: that Quickshell loses a pending save. Hyprland 0.57.1 in place of sway
(`hl.dsp.exit()`, SIGTERM, SIGKILL) was measured by hand in the guest of `bench/vm.sh`: the
first three rows are the same for both.

How long Quickshell's `Process`es stay, in Hyprland runs of the same three kinds, five runs
of each: they have no parent when Quickshell ends, whichever way Hyprland went, 5 of 5.
Looked at again 2 s later, they are gone after `hl.dsp.exit()` and after SIGTERM, 5 of 5
each, and still run after SIGKILL, 5 of 5. What ends them in the first two was not looked
for. impulse-shell leaves none when it ends in any of the three, 5 of 5 each. In these runs both
shells ended with 255, 53 to 147 ms after the compositor.

## A picture sent over D-Bus larger than 1024 pixels a side

Compared with Quickshell 0.3.2 under sway, through OpenGL (`crates/shell/tests/tray_pixels_qt.rs`).

| | Quickshell 0.3.2 | impulse-shell |
|---|---|---|
| A tray pixmap (`IconPixmap`, `OverlayIconPixmap`, `AttentionIconPixmap`) or a notification's `image-data` of up to 1024x1024 | Kept as it came: one of 512x512 is shown pixel for pixel. | The same, 0 of 262144 pixels differ. |
| One with a side over 1024 | Kept as it came: one of 1025x1025 is shown pixel for pixel, and the notification's `Image` is 1025x1025. | Averaged down once, when it arrives, so that its longer side is 1024 (1025x1025 becomes 1024x1024, 2048x1024 becomes 1024x512). The notification's `Image` is 1024x1024. |

Why: the sender decides the size, and the picture is held for as long as the tray item or
the notification lives. With the limit one picture holds at most 1024 x 1024 x 4 bytes =
4 MiB. Without it the only bound is D-Bus's own: 64 MiB for one array, and the hints of a
notification are one array around the picture, so the picture's bytes can be 64 MiB less 60
bytes (measured: 67108804 bytes arrive, 67108808 close the sender's connection). That is a
picture of 4096x4095, 16 times as much for each picture, and any program on the session bus
can send one.

A picture of 4096x4096 depends on the bus (measured with libnotify as the sender,
`notify_notification_set_image_from_pixbuf`):

| Bus | Quickshell 0.3.2 | impulse-shell |
|---|---|---|
| `dbus-daemon` 1.16 with the session's configuration | The bus closes the sender's connection; nothing arrives. | The same. |
| `dbus-broker` 37 | The bus passes it on; Quickshell's connection to the bus then ends: the sender gets `Remote peer disconnected`, `org.freedesktop.Notifications` has no owner afterwards and the next notification fails with `ServiceUnknown`. `VmRSS` 191812 kB before, 264756 kB after. | Arrives and is kept as 1024x1024 (`VmRSS` 156688 kB before, 179828 kB after); the next notification arrives. So does one of 5792x5792 (134189056 bytes); one of 5800x5800 is over the broker's 128 MiB for a message and closes the sender's connection, the shell stays on the bus. |

Measured, Quickshell 0.3.2 under sway on llvmpipe, `VmRSS` before and after one notification
with a picture of 4096x4095: 190308 kB and 412260 kB when the picture is shown (217 MiB
more), 191532 kB and 264848 kB when it is only kept (72 MiB more).

The bus has to let the message through first. `dbus-daemon` takes a message of at most
32 MiB (33554432 bytes) unless its configuration sets `max_message_size`; the session's
(`/usr/share/dbus-1/session.conf`) sets 1000000000. A picture of 4096x2048 is a message of
33554736 bytes: the session bus delivers it (libnotify as the sender, through `dbus-daemon`
with the session's configuration and through `dbus-broker`: Quickshell shows 4096x2048, impulse-shell
keeps 1024x512), a `dbus-daemon` with no such line closes the sender's connection, for
Quickshell as for impulse-shell.

Not held by a test: the rows for `dbus-broker` and for libnotify (measured by hand in the
test image; the tests' bus is `dbus-daemon` and their sender zbus).

Tests: `pictures_over_1024_are_averaged`, `a_notification_picture_of_512` and
`tray_pixmaps_of_every_size` in `crates/shell/tests/tray_pixels_qt.rs` (both sides);
`a_pixmap_is_kept_up_to_1024_a_side` in `crates/qml/src/sni.rs` and
`an_image_is_kept_up_to_1024_a_side` in `crates/qml/src/notifications.rs` (1024 kept, 1025
averaged); `oversized_image_kept_downscaled` and `an_image_of_32_mib_is_kept_downscaled`
in `crates/qml/tests/notifications.rs`.

## Qt's scale variables

Compared with Quickshell 0.3.2 on Qt 6.12.0 under sway with two outputs, at scale 1 and 1.5.
`QT_SCALE_FACTOR` and `QT_SCREEN_SCALE_FACTORS` do in impulse-shell what they do in Quickshell
([env.md](env.md#qts-scale-variables)); the rows below are what differs. All but the popup
are differences from Qt itself and are in
[qt-differences.md](qt-differences.md#qts-scale-variables) as well, with what `qml6` shows.

| | Quickshell 0.3.2 | impulse-shell |
|---|---|---|
| `QT_SCALE_FACTOR` that is a number but not a finite one above 0; an infinite entry of `QT_SCREEN_SCALE_FACTORS` | Taken as the factor, as Qt does: no window appears, and nothing is logged about the variable; with a visible `PopupWindow` Quickshell ends on a protocol error. The values are in the table below. | The factor is 1, the entry is dropped, and one line in the log names the variable and the value. |
| `QT_FONT_DPI=N` | The whole interface is scaled by N/96 (192 doubles it), not only fonts; multiplied with `QT_SCALE_FACTOR`; a screen's entry of `QT_SCREEN_SCALE_FACTORS` replaces it on that screen. | Not read. |
| `QT_WAYLAND_FORCE_DPI=N` | The same as `QT_FONT_DPI=N`. | Not read. |
| `QT_WAYLAND_FORCE_DPI=physical`, `QT_USE_PHYSICAL_DPI=1` | The factor comes from the output's physical size (`QWaylandScreen::logicalDpi`, `QPlatformScreen::overrideDpi`). On headless outputs, which have no physical size: nothing, and 100/96 for `QT_USE_PHYSICAL_DPI`. Not checked on a monitor. | Not read. |
| `QT_ENABLE_HIGHDPI_SCALING=0` | The factor from the DPI above no longer scales the interface (with `QT_FONT_DPI=192` a 12pt font is 32 px instead); `QT_SCALE_FACTOR` and `QT_SCREEN_SCALE_FACTORS` still apply. | Not read. |
| `QT_SCALE_FACTOR_ROUNDING_POLICY=Round` (and the other policies) | Rounds the factor from the DPI and the entries of `QT_SCREEN_SCALE_FACTORS` (`1.3;1.6` is 1 and 2), not `QT_SCALE_FACTOR`. A window on the output at scale 1.5 then has a ratio of 4 with the entry 1.6, not 3. | Not read: the entries are taken as they are (Qt's default, `PassThrough`). |
| A `PopupWindow` of a window on a screen that is not at 0,0 of the compositor's layout, when that screen's factor (`QT_SCALE_FACTOR` times its entry of `QT_SCREEN_SCALE_FACTORS`) is not 1 | Quickshell moves the popup off its anchor by the screen's position in the layout times (1 − factor); the compositor then keeps it on the output, so with a factor above 1 it is at the output's left edge. Its size is in the factor of the screen at 0,0, not of its own. The sizes are in the table below. | The popup is at its anchor, sized by the factor of its parent's screen. |
| CPU rendering (`RI_RENDER=cpu`) on a compositor without `wp_viewporter` | There is no such renderer: a factor always enlarges the window. | The factor does not enlarge the window. |

The values no window can have. "Nothing logged" is: no line that a run without the
variable does not have. Measured with a config of bars and floating windows. With a visible `PopupWindow` as well, Quickshell ends at
each value of the rows with a warning: Qt asks for a popup without a size, and the log has
`error 0: width and height must be positive and non-zero` and
`WARN: The Wayland connection experienced a fatal error: Protocol error`.

| Value | The factor in Quickshell's QML | What Quickshell shows and logs | impulse-shell |
|---|---|---|---|
| `QT_SCALE_FACTOR=0`, `-0` | 0 | No bar on the first output, a strip 2 px high on the second. Nothing logged. | 1, one warning |
| `QT_SCALE_FACTOR=-1` | −1 | No window. Nothing logged. | 1, one warning |
| `QT_SCALE_FACTOR=nan`, `NaN` | NaN | No window. Qt logs `The cached device pixel ratio value was stale on window expose` and `… on window update`. | 1, one warning |
| `QT_SCALE_FACTOR=inf`, `+inf`, `INF`, `-inf` | ±Infinity | No window. Nothing logged. | 1, one warning |
| `QT_SCALE_FACTOR=` (empty), `abc`, `2,5`, `2x`, `1e999`, `infinity` | 1 | As without the variable. Nothing logged. | the same |
| `QT_SCREEN_SCALE_FACTORS=X;2` with X one of `0`, `-1`, empty, `abc`, `nan`, `-inf`, `1e999` | the entry is dropped: the first screen has 2, the second 1 | Nothing logged. | the same |
| `QT_SCREEN_SCALE_FACTORS=HEADLESS-1=X;HEADLESS-2=2` with X one of `0`, `-1`, `abc`, `nan` | the entry is dropped: `HEADLESS-1` has 1, `HEADLESS-2` 2 | Nothing logged. | the same |
| `QT_SCREEN_SCALE_FACTORS=inf;2`, `HEADLESS-1=inf;HEADLESS-2=2` | Infinity on the first screen, 2 on the second | No window on the first screen. Nothing logged. | the entry is dropped (`inf;2` then gives the first screen 2), one warning |

The warnings:

```
WARN: QT_SCALE_FACTOR=nan is not a scale factor a window can have: the factor is 1
WARN: QT_SCREEN_SCALE_FACTORS: the entry HEADLESS-1=inf is not a scale factor a window can have: the entry is dropped
```

The popup, measured in sway with `HEADLESS-1` (1920x1080, scale 1) at 0,0 and `HEADLESS-2`
(2560x1600, scale 1.5) at 1920,0. Each screen has a bar at its top with a left margin of 20
and a `PopupWindow` of 50x30 anchored at 100,40 of the bar. The popup of the bar on
`HEADLESS-2`, in that output's pixels (x y width height):

| Session | Quickshell 0.3.2 | impulse-shell |
|---|---|---|
| neither variable | 180 60 75 44 | 180 60 75 44 |
| `QT_SCALE_FACTOR=2` | 0 120 150 89 | 360 120 150 89 |
| `QT_SCREEN_SCALE_FACTORS=1.5;2` | 0 120 113 67 | 360 120 150 89 |
| `QT_SCREEN_SCALE_FACTORS=1;2` | 0 120 75 44 | 360 120 150 89 |

The popup of the bar on `HEADLESS-1`, the screen at 0,0, is at its anchor in both shells.
With the outputs swapped in the layout the popup on `HEADLESS-1` is the misplaced one.

The same with the original itself (`c33d2c5f` on Quickshell 0.3.2) was run once by hand in the QEMU
guest under Hyprland and is not held by a test: `Virtual-1` (1920x1080, scale 1) at 0,0, a
headless output of 2560x1600 at 1920,0 at scale 1.6 (Hyprland does not take 1.5 for that
mode), the Waffle bar, `QT_SCALE_FACTOR=2`. The tooltip of the Start button on the second
output (`WPopupToolTip`, a `PopupWindow`) is at the output's left edge in the original and above its
button in impulse-shell. On `Virtual-1` with the same factor, and on the second output with
`QT_SCALE_FACTOR=1`, it is above the button in both.

Why: a shell without a window and without a word about the variable in its log is a
breakage, not a behaviour to repeat, so the shell keeps its windows and says what it did
not take. A popup away from
the button that opened it is unusable on the second monitor: that is a bug of Quickshell
0.3.2. The five variables that are not read are not used by the original's configs; they are left for
a change of their own.

Tests:

- `a_qt_scale_factor_no_window_can_have_is_one_and_one_warning` and
  `an_infinite_screen_factor_is_dropped_and_one_warning` in
  `crates/shell/tests/scale_factor_equiv.rs` hold the rows with a warning: Quickshell's
  factor and silence, the native shell's windows and its one line.
  `a_qt_scale_factor_that_is_no_number_is_one` and
  `a_screen_factor_that_is_no_number_above_zero_is_dropped` hold the rows where both are
  silent. `crates/shell/src/hidpi.rs` has the same values without a compositor.
- `a_popup_on_another_screen_stays_at_its_anchor` in the same file holds both columns of
  the popup row, for `QT_SCALE_FACTOR=2` and `QT_SCREEN_SCALE_FACTORS=1.5;2`.
- `the_other_scale_variables_are_not_read` in the same file holds the five variables that
  are not read: the native shell shows with each what it shows without, and Quickshell's
  factors for `QT_FONT_DPI=192`, `QT_WAYLAND_FORCE_DPI=192` and
  `QT_SCALE_FACTOR_ROUNDING_POLICY=Round`. `QT_ENABLE_HIGHDPI_SCALING`, the physical DPI
  and the CPU renderer without `wp_viewporter` are not held by a test.
- The other tests of `crates/shell/tests/scale_factor_equiv.rs` hold that the two variables
  read in impulse-shell as in Quickshell.

An output connected while the shell runs, with `QT_SCALE_FACTOR=2`, was run once by hand in
sway and is not held by a test: the new output's bar, its size after the output's mode and
scale change, and its removal with the output are the same in both shells, but for the
popup above.

Not checked: touch and tablet input; under Hyprland, anything but that tooltip.

## The environment of the programs the shell starts

Compared with Quickshell 0.3.2. Quickshell copies its environment before it applies the
config's pragmas (`Common::INITIAL_ENVIRONMENT`, `src/launch/launch.cpp`) and starts `Process`,
`Process.startDetached()` and `Quickshell.execDetached` with that copy
(`setupProcessEnvironment`, `src/io/processcore.cpp`): `//@ pragma Env`, `//@ pragma DefaultEnv`,
the `Fusion` style and the removal of `QT_STYLE_OVERRIDE` reach the shell alone. impulse-shell does the
same (`workpool::userenv`), also for the programs it starts from Rust where the original started them
from QML or from a script QML started. The rows below are what differs.

| | Quickshell 0.3.2 | impulse-shell |
|---|---|---|
| The PAM conversation | A `fork()` without `exec` of the shell (`src/services/pam/subprocess.cpp:40`): it has the shell's environment, pragmas included. | A helper process, `impulse-shell` itself, started with the shell's environment, pragmas included. |
| The polkit agent's helper | Started by polkit's library inside the shell: the shell's environment. | `polkit-agent-helper-1` started with the shell's environment. |
| The guard of the children's process groups | No such process. | `impulse-shell` itself with the shell's environment; it reads no variable. |

Why: the first two rows keep what Quickshell's code in the shell's own process sees.

Tests:

- `crates/shell/tests/child_env_equiv.rs` holds for `Process`, `Process` with `environment`,
  `Process.startDetached()` and `Quickshell.execDetached` that both shells give the same
  environment, with and without the variables in the session, and after a reload.
- `the_shell_s_own_programs_get_the_user_s_environment` (`crates/qml/src/userenv_sites.rs`)
  and `the_theme_s_programs_get_the_user_s_environment`
  (`crates/colorgen/src/userenv_sites.rs`) hold it for the programs started from Rust.

Not checked: the three rows above are from the code and are not held by a test.

## A popup shown while its panel's output is removed

Compared with Quickshell 0.3.2 under sway 1.12 (wlroots 0.20.2).

| | Quickshell 0.3.2 | impulse-shell |
|---|---|---|
| The output of a panel is removed after the client wrote `zwlr_layer_surface_v1.get_popup` and before the compositor read it | The process ends with status 255: QtWayland commits the popup right after `get_popup`, the compositor answers with the protocol error `xdg_popup has no parent` and closes the connection, and `QWaylandDisplay::checkWaylandError` calls `_exit(-1)`. Its last line is one of two, see below. | The shell stays. The panel and the popup are closed, the popup without a commit. |
| First commit of a panel's popup | In the same write as `get_popup`. | One round trip later: after the answer to a `wl_display.sync` sent with `get_popup` (`PopupAdopted`). |

Why: wlroots ends the layer surface of a removed output: it sends `closed` and makes the
resource inert ([`wlr_layer_surface_v1_destroy`][wl-destroy]). `get_popup` on an inert layer
surface does nothing ([`layer_surface_handle_get_popup`][wl-get-popup]), so the popup stays
without a parent, and a commit of such a popup is the protocol error `xdg_popup has no
parent` ([`handle_xdg_popup_client_commit`][wl-commit]). A protocol error ends the
connection, and the shell with it. The client cannot know that the layer surface has ended
until it reads `closed`, but `closed` always comes before the answer to a `wl_display.sync`
sent after `get_popup`. So impulse-shell commits the popup on that answer, and only if the panel is
still there.

The other order, the layer surface ending after the compositor read `get_popup` and before it
reads the popup's first commit, is run by
`popup_committed_while_its_panels_screen_is_unplugged_does_not_end_the_shell`: the test lets
`get_popup` and the sync through, holds the commit the shell writes on the answer, removes the
output and lets go. The shell stays under both compositors (20 of 20 under sway with 16 busy
processes):

| | sway 1.12 (wlroots 0.20.2) | Hyprland 0.57.1 |
|---|---|---|
| Sent to the shell when the output goes | `zwlr_layer_surface_v1.closed`, then `xdg_popup.popup_done` 0.02 ms later: wlroots destroys the popup with its layer surface ([`layer_surface_reset`][wl-reset], [`wlr_xdg_popup_destroy`][wl-popup-destroy]). | `closed` alone, no `popup_done`. |
| The held commit, read after that | Read and ignored ([`wlr_xdg_surface.c` 111-116][wl-ack], [307-311][wl-surface-commit]). No `wl_display.error`. | Read. No `wl_display.error`; nothing about the popup in Hyprland's log. |
| The shell | Destroys the popup, `PANELCLOSED`, shows the next panel. | The same. |

The test's last line asks for `popup_done`, so it does not pass under Hyprland (0 of 20, each
at that line and nowhere before it). It was not red before the change: without the change the
commit is in the same write as `get_popup`, so this order cannot be made.

Cost, measured under sway on one machine, 20 popups each way, times from the `get_popup`
request:

| | Before | After |
|---|---|---|
| First commit of the popup, median (min to max) | 0.02 ms (0.02 to 0.04) | 0.35 ms (0.18 to 4.40) |
| `configure` of the popup, median (min to max) | 0.37 ms (0.27 to 1.86) | 0.45 ms (0.25 to 4.51) |
| First frame: `wl_surface.enter` of the popup, which sway sends when its first buffer is on the output, median | 41 ms | 36 ms |

The 0.33 ms added to the first commit is the wait for `wl_callback.done`: the shell's write
of `get_popup` and `wl_display.sync`, sway's read and answer, and the shell's loop waking on
it. The first frame is not later because that wait is not what it waits for: after
`configure` the shell draws the popup's first buffer, and its `attach` comes 28 to 86 ms
after `get_popup` (before the change; `enter` follows about 1 ms later). 0.33 ms is about 1%
of the shortest of these, and the first frame comes 24 to 87 ms after `get_popup` both ways,
so 41 ms and 36 ms are two medians of the same spread, not a gain.

The same under Hyprland 0.57.1 in the benchmark guest, 40 popups each way (two rounds of 20,
taken in turn), medians:

| | Before | After |
|---|---|---|
| First commit of the popup (min to max) | 0.02 ms (0.02 to 0.05) | 0.75 ms (0.48 to 10.24) |
| `configure` of the popup (min to max) | 0.63 ms (0.41 to 9.63) | 0.69 ms (0.43 to 10.19) |
| `attach` of the popup's first buffer | 1.08 ms | 1.15 ms |
| `wl_surface.enter` of the popup | 1.77 ms | 1.90 ms |

The first commit is 0.73 ms later and the first buffer 0.07 ms later: Hyprland sends the
popup's `configure` in the same write as the answer to the sync, and the shell needs the
`configure` before it can attach a buffer either way. The four rounds, in ms, before and
after: first buffer 1.06 and 1.09 against 1.15 and 1.14, `enter` 1.80 and 1.75 against 1.85
and 1.92.

The other places where the shell makes an object on a parent the compositor can end, each
run the same way (the compositor's reading held, the parent ended, the reading let go) under
sway:

| Object, request | Parent the compositor can end | The same race |
|---|---|---|
| Popup of a panel, `zwlr_layer_surface_v1.get_popup` | The layer surface: `closed`. | Yes. Closed by this change. |
| Popup of a window, `xdg_surface.get_popup` with the window's `xdg_surface` | None. The compositor only asks a window to close (`xdg_toplevel.close`); the `xdg_surface` is the client's to destroy. | No. Run: the window was killed while the popup's requests were held; the shell stays. |
| Popup of a popup, `xdg_surface.get_popup` with the parent popup's `xdg_surface` | The parent popup: `popup_done`. Its `xdg_surface` stays and keeps its role. | No: wlroots takes the parent's surface whether or not its popup is gone ([`create_xdg_popup`][wl-create-popup]). Run: the output of the panel under a shown popup was removed while the nested popup's `get_popup` was held; the shell stays. A nested popup shown in the same turn as its parent is asked for only after the parent, so it is not in the held write at all. |
| Tray menus and other menus | They are `PopupWindow` items and go through the same one place in the shell (`sync_windows`), so one of the three rows above by their parent. The tray menu of the bar (`SysTrayMenu.qml`) is a popup of a panel. | As that row. Run for the tray menu: open when the output is removed, it is closed with the panel; opened while the output is removed, it gets no commit. The shell stays in both. |
| Lock surface, `ext_session_lock_v1.get_lock_surface` | The lock (`finished`) and the output. | No: wlroots makes the lock surface inert in both cases, "to stay in sync with the client" ([`lock_handle_get_lock_surface`][wl-lock-surface]). Run: the output was removed while `get_lock_surface` was held; the shell stays. |
| Panel, `zwlr_layer_shell_v1.get_layer_surface` with an output | The output. | No protocol error: the panel gets `closed`. See "A panel whose output is removed" above. |
| Subsurface, `wl_subcompositor.get_subsurface` | None: the parent is the shell's own `wl_surface`. | No. |

Waiting for the answer to a `wl_display.sync` is what the
session lock does after `unlock_and_destroy` (`UnlockSync`); `PopupAdopted` shares no code
with it beyond the `wl_callback` handler each of them has.

Quickshell's last line. sway sends the error and closes the connection, and two threads of
QtWayland read it. 600 runs with 16 busy processes on 8 cores:

| Runs | Last line of Quickshell | Which thread got there first |
|---|---|---|
| 585 | `The Wayland connection experienced a fatal error: Protocol error` | The one that reads the `wl_display.error` event: libwayland sets `EPROTO`. |
| 15 | `The Wayland connection broke. Did the Wayland compositor die?` | The one that reads the end of the stream: libwayland sets `EPIPE` ([`read_events`][wl-read-events]) and runs no event after an error is set ([`dispatch_queue`][wl-dispatch-queue]). In 5 of the 15 the `wl_display.error` event is not in Quickshell's `WAYLAND_DEBUG` log either. |

Both lines are from [`QWaylandDisplay::checkWaylandError`][qt-check-error], which then calls
`_exit(-1)`. The status is 255 in all 600, and sway sent `xdg_popup has no parent` in all 600
(read from the socket between the two).

Tests, in `crates/shell/tests/windows_outputs.rs`. Each holds the compositor's reading of
the client's requests until the parent has ended:

- `popup_shown_while_its_panels_screen_is_unplugged_does_not_end_the_shell` checks the
  impulse-shell column of the first row.
- `quickshell_ends_when_a_popup_is_shown_while_its_panels_screen_is_unplugged` checks the
  Quickshell column of the first row: status 255, the error sway sent, and one of the two
  last lines above.
- `popup_shown_while_its_window_is_asked_to_close_does_not_end_the_shell`,
  `popup_of_a_popup_shown_while_its_panels_screen_is_unplugged_does_not_end_the_shell` and
  `lock_surface_asked_for_a_screen_being_unplugged_does_not_end_the_shell` check the rows of
  the table above for a popup of a window, a popup of a popup and a lock surface: the shell
  shows a panel afterwards and got no protocol error.
- `popup_committed_while_its_panels_screen_is_unplugged_does_not_end_the_shell` runs the
  other order (above).
- `tray_menu_open_when_its_panels_screen_is_unplugged_is_closed` and
  `tray_menu_opened_while_its_panels_screen_is_unplugged_does_not_end_the_shell` run the
  tree's tray menu through both: the menu is closed (`visible` is false), the panel is
  closed, the shell shows a panel afterwards.

Hyprland 0.57.1 sends no error here, so Quickshell does not end there at all. By its sources: a
layer surface of a removed monitor gets `closed` and its resource stays (`Monitor.cpp`),
`get_popup` on it fails only for a popup that already has a parent (`LayerShell.cpp`), and a
commit of an `xdg_surface` has no "no parent" error (`XDGShell.cpp`). Run in the benchmark
guest (`bench/vm.sh`; Hyprland 0.57.1, Quickshell 0.3.2, Qt 6.12.0) with 16 busy processes,
`swaymsg` replaced by a script that says the same to `hyprctl` (`output create headless`,
`output remove`, `hl.dsp.window.close`):

| | Quickshell 0.3.2 | impulse-shell | impulse-shell before the change |
|---|---|---|---|
| Requests held | `get_popup`, `commit` of the popup | `get_popup`, `wl_display.sync` | `get_popup`, `commit` of the popup |
| After `closed` | Destroys the popup and the panel. No `wl_display.error`; the process is still there 30 s later, so its test, which waits for the end, does not pass on Hyprland: 100 of 100 runs under 16 busy loops reached the 30 s limit with `qs` alive and no `wl_display.error`. | Destroys the popup without a commit, shows the next panel. Its test passes 100 of 100. | The shell stays and shows the next panel, but the popup of the closed panel was committed: the test fails at that line, 20 of 20. |

The tests of the other parents under Hyprland, 20 runs each with the same load: a popup of a
window 20, a popup of a popup 20, a lock surface 20, the tray menu open 20, the tray menu
being opened 20.

So on Hyprland the round trip takes away a commit Hyprland does not check; the shell stayed
there before the change too. It is there for wlroots compositors.

[wl-destroy]: https://gitlab.freedesktop.org/wlroots/wlroots/-/blob/0.20.2/types/wlr_layer_shell_v1.c#L337-343
[wl-get-popup]: https://gitlab.freedesktop.org/wlroots/wlroots/-/blob/0.20.2/types/wlr_layer_shell_v1.c#L240-250
[wl-commit]: https://gitlab.freedesktop.org/wlroots/wlroots/-/blob/0.20.2/types/xdg_shell/wlr_xdg_popup.c#L247-252
[wl-reset]: https://gitlab.freedesktop.org/wlroots/wlroots/-/blob/0.20.2/types/wlr_layer_shell_v1.c#L33-40
[wl-popup-destroy]: https://gitlab.freedesktop.org/wlroots/wlroots/-/blob/0.20.2/types/xdg_shell/wlr_xdg_popup.c#L486-498
[wl-ack]: https://gitlab.freedesktop.org/wlroots/wlroots/-/blob/0.20.2/types/xdg_shell/wlr_xdg_surface.c#L111-116
[wl-surface-commit]: https://gitlab.freedesktop.org/wlroots/wlroots/-/blob/0.20.2/types/xdg_shell/wlr_xdg_surface.c#L307-311
[wl-create-popup]: https://gitlab.freedesktop.org/wlroots/wlroots/-/blob/0.20.2/types/xdg_shell/wlr_xdg_popup.c#L420-422
[wl-lock-surface]: https://gitlab.freedesktop.org/wlroots/wlroots/-/blob/0.20.2/types/wlr_session_lock_v1.c#L222-248
[wl-read-events]: https://gitlab.freedesktop.org/wayland/wayland/-/blob/1.26.0/src/wayland-client.c#L1761-1767
[wl-dispatch-queue]: https://gitlab.freedesktop.org/wayland/wayland/-/blob/1.26.0/src/wayland-client.c#L1866-1867
[qt-check-error]: https://github.com/qt/qtbase/blob/v6.12.0/src/plugins/platforms/wayland/qwaylanddisplay.cpp#L579-L592

## A reload that cannot happen

Quickshell 0.3.2 loads the next configuration inside its process, beside the running one,
and keeps its Wayland connection. impulse-shell re-executes itself (`exec`), so the running tree is
gone before the next one loads. What would leave no shell is therefore looked for before the
`exec`; when it is found the running tree stays as it is, no `Component.onDestruction` runs,
and `Quickshell.reloadFailed(error)` is emitted, which the original's `ReloadPopup.qml` shows. It is
emitted twice, as Quickshell emits it twice for one failed reload (`RootWrapper::reloadGraph`
in `src/core/rootwrapper.cpp` has the `emit` before and after it decides on its own popup).

| | Quickshell 0.3.2 | impulse-shell |
|---|---|---|
| A reload that happens | The new tree completes and gets `reloadCompleted`, then `Component.onDestruction` of the old one runs. | `Component.onDestruction` of the old tree runs, then the new image loads the new tree. The reload shows: the old image's windows above the other windows (the top and overlay layers) fade away before the `exec` (0.4 s, `shell::reload::FADE_OUT`), and the new image's come as at a start, seen from their first frame, with the compositor's animation of a new layer and the tree's own first animations; the old wallpaper stays until the new one, unseen while it draws, takes its place in one frame. Quickshell's windows go and come as the compositor animates them. The new tree gets `reloadCompleted` when its windows are all there: `ReloadPopup.qml` shows from then, not from the load. On Hyprland the compositor is asked to animate none of the shell's layers from the moment the new image ends the old ones (from its start, where the old ones did not fade: no wp_alpha_modifier_v1) until it has closed them (a layer rule `no_anim` for `rustshell` and `rustshell:*`, `shell::hyprclose::still`): it would animate each closed layer away on a snapshot over the new one. |
| The tree differs from the one compiled in (a file edited, added or removed), build without the interpreter | No such case: it compiles what is there. Its text for a configuration that does not load is `Failed to load configuration` and one `  caused by @<file>[<line>:<column>]: <message>` line per error. | Refused. The text is `<tree> differs from the tree this shell was built from. The running shell keeps its current code.`, one line per file (`<file>: <why>`, at most five), and `Install again: tools/install.sh`. |
| The compositor's socket is gone | Reloads: the connection it has stays. | Refused: `Cannot connect to the Wayland compositor: <why>` and `The running shell keeps its current code.` |

The start of a build without the interpreter refuses the same trees as its reload and as
`--check-tree` (`shell::reload::differing_tree`): status 1 and `impulse-shell: install again:
tools/install.sh`, also for a tree that only lacks a compiled file.

What is left: the socket is looked at before the `exec` and connected to after it. A
compositor that goes in between ends the shell (status 1). With the socket removed before
the reload is asked for, the running tree stayed in 20 runs of 20.

Tests:

- `reload_ends_the_old_tree` and `reload_with_the_socket_gone_keeps_the_running_tree` in
  `crates/shell/tests/reload_equiv.rs` run both shells for the first and the third row.
- `a_reload_on_an_edited_tree_keeps_the_shell_and_shows_why` and
  `a_reload_on_a_tree_that_lacks_a_file_keeps_the_shell_and_shows_why` in
  `crates/shell/tests/shipped_panels.rs` press the original's reload button in the installed shell.
- `a_tree_that_lacks_a_compiled_file_is_refused` in `crates/shell/tests/startup_errors.rs`
  checks the start and `--check-tree`.

## A changed window whose frame callback is not answered

Not deliberate: found while the install was being prepared and left as it is until after
it. To do what Quickshell does the frame path needs a wait of 100 ms by the wall clock, and
a compositor under load answers later than that on its own.

Compared with Quickshell 0.3.2 (Qt 6.12.0) under sway 1.12, by the requests each sends
(`WAYLAND_DEBUG`), four runs of each. The shell is a panel with an endless animation and a
`WlSessionLock`; a proxy between the shell and sway keeps back the answer to the panel's
frame callback until the shell sends `unlock_and_destroy`, as a locked sway does for a
window it no longer shows.

| | Quickshell 0.3.2 | impulse-shell |
|---|---|---|
| The panel has changed and the answer to its last frame's callback does not come | Commits one more frame of the panel 100 ms after the last one (100 and 101 ms in the runs), without a new frame callback request. After it no frame of the panel: 2.9 s in the run that was locked that long. | No frame of the panel until the answer comes: 3.0 s in the same run. |
| The session is locked 15 ms after the panel's last frame, the answer not there | The lock surface is asked for and has its first frame 55 ms after the `lock` request; that one frame of the panel follows 31 ms later. | The lock surface is asked for and has its frame; no frame of the panel. |
| After `unlock_and_destroy` and the answer | The panel draws frame after frame again, the first 98 ms later. | The same, the first 7 ms later. |

Why Quickshell sends that frame, read in Qt 6.12.0 and not measured: the render thread has
the next frame ready and waits for the frame callback before it swaps, for 100 ms at most
([`QWaylandGLContext::swapBuffers`][qt-swap-wait] calls `waitForFrameSync(100)`); then it
swaps, and the window counts as not exposed until the callback comes
(`mFrameCallbackTimedOut`). impulse-shell takes Qt's 100 ms for the animations only
(`FRAME_CALLBACK_TIMEOUT_MS`, [clocks.md](clocks.md)): they stop waiting, the window still
draws nothing.

[qt-swap-wait]: https://github.com/qt/qtbase/blob/v6.12.0/src/plugins/platforms/wayland/plugins/hardwareintegration/wayland-egl/qwaylandglcontext.cpp#L506

What a user sees is the same: the frame goes to a window the compositor does not show
(under the lock screen, on an output that is powered off), and once it is shown again both
draw its current state.

`a_window_awaiting_its_frame_callback_when_locked_draws_again_once_unlocked` in
`crates/shell/tests/session_lock_finished.rs` holds what both do: the lock screen gets its
frame, the lock is taken and released, and the panel draws again after the answer. It does
not hold the number of the panel's frames while the answer is kept back.

# Where impulse-shell follows Quickshell and not Qt

The QML engine does what Qt 6.12 does. The cases below are the ones where Quickshell 0.3.2
itself does something else with the same files, and impulse-shell does what Quickshell does.

## A `FrameAnimation` behind a lock or on a dark screen

A `FrameAnimation` is triggered at every step of the animation clock, in a window that is
shown and in one that is not, here as under Quickshell
(`crates/shell/tests/frame_animation_equiv.rs`). One that runs keeps that clock stepping, on
its timer where no frame callback comes: 60 or more wakeups of the main thread a second for
as long as it runs.

| | Quickshell 0.3.2 | impulse-shell |
|---|---|---|
| The compositor has confirmed the session lock; the `FrameAnimation` is in a window that is no lock surface | Triggered at every step, as before the lock. | Not stepped: it waits for the unlock, like an endless animation of a window that is not shown. A turn of the loop that something else asks for (a Timer, an event) still triggers it once. |
| Every output is powered off (`zwlr_output_power_v1`); any `FrameAnimation`, also one of a lock surface or of no window | Triggered at every step. | Not stepped, until the first output is on. |
| The session unlocked and an output on | Triggered at every step. | The same, from the first frame after the unlock or the output's power. `frameTime` of that first trigger is the time it rested. |

Deliberate: nobody can see what such a step draws, and the compositor has said so. Measured
in a guest (Hyprland 0.57.1, music playing, the original's media controls open, whose progress bar
waves on a `FrameAnimation`), main thread, session locked and the screen off: Quickshell
1.33 s of CPU a minute and 135 wakeups a second, impulse-shell 0.34 s and 5.5 (4.74 s and 129 with
the visualizer capturing and the `FrameAnimation` stepped).

Without `zwlr_output_power_manager_v1` every output counts as on, and only the lock rests
them. For this the shell keeps one `zwlr_output_power_v1` for each output while it runs.
Sway (wlroots) gives an output's to one client at a time and answers `failed` to the next:
another client's request for it fails while impulse-shell runs, and impulse-shell counts an output as on
whose `zwlr_output_power_v1` another client holds. Hyprland gives one to every client that
asks.

Code: `sync_unseen`, `out_of_sight` and `animating_windows` in `crates/shell/src/main.rs`.

Test: `frame_animations_rest_out_of_sight` in `crates/shell/tests/visualizer_off_screen.rs`.

## A directory with no `qmldir`

Quickshell writes the `qmldir` of every directory it loads that has none, with a `singleton`
line for each file that starts with `pragma Singleton`. Qt alone registers no singleton for
such a directory. the original's tree has no `qmldir` files and uses its singletons by name, so impulse-shell
does the same as Quickshell (`read_dir_types` in `crates/qml/src/resolve.rs`). A directory
that has a `qmldir` follows Qt's rules in both.

`d/Solo.qml` starts with `pragma Singleton` and has `property int v: 7`; the document has
`import "d"`:

| | Qt 6.12 | Quickshell 0.3.2 and impulse-shell |
|---|---|---|
| `Solo.v` | `undefined` | `7` |
| `typeof Solo` | `object` | `object` |
| `Solo {}` | `Type Solo unavailable`, `pragma Singleton used with a non composite singleton type Solo` | `Composite Singleton Type Solo is not creatable.` |

Test: `a_directory_without_qmldir_has_the_singletons_quickshell_gives_it` in
`crates/qml/tests/import_namespace.rs`, with the values of both in
`crates/qml/tests/data/no_qmldir/quickshell.txt`.

## The url of a document in the shell directory

Quickshell loads the documents of the shell directory through its own scheme: such a document
has the url `qs:@/qs/<path in the directory>`, where Qt alone, given the file, has
`file:///<path>`. impulse-shell does the same as Quickshell (`World::ctx_url` in
`crates/qml/src/runtime.rs`). `baseUrl` of a `Text`, `TextArea` or `TextEdit` nothing was
written to is the url of the document that holds the object, so it shows the difference. A
document outside of the shell directory that is loaded by a `file:` url has that url in all
three; loaded by its path, it has `qs:<path>` in Quickshell and impulse-shell.

`Probe.qml` and `sub/Sub.qml` are in the shell directory `DIR/cfg`, `DIR/out/Out.qml` is not:

| `baseUrl` of a `Text` or `TextArea` in | Qt 6.12 | Quickshell 0.3.2 and impulse-shell |
|---|---|---|
| `Probe.qml` | `file://DIR/cfg/Probe.qml` | `qs:@/qs/Probe.qml` |
| `sub/Sub.qml` | `file://DIR/cfg/sub/Sub.qml` | `qs:@/qs/sub/Sub.qml` |
| `Out.qml`, loaded as `file://DIR/out/Out.qml` | `file://DIR/out/Out.qml` | `file://DIR/out/Out.qml` |
| `Out.qml`, loaded as `DIR/out/Out.qml` | `file://DIR/out/Out.qml` | `qs:DIR/out/Out.qml` |

Test: `base_url_is_quickshell_s` in `crates/shell/tests/qs_urls.rs`, with what Quickshell
prints in `crates/shell/tests/qs_base_url/expected.qs`. `crates/shell/tests/qt_props` compares
with Qt alone, so it lists `baseUrl` of `Text` and `TextArea` as differing
(`qt_props/differ/Text.txt`, `TextArea.txt`).

## Code inside Markdown and LaTeX code blocks

Compared with Quickshell 0.3.2 and KSyntaxHighlighting 6.31.0, on the code block of the original's
assistant chat (`qml/modules/rusty/sidebarLeft/aiChat/MessageCodeBlock.qml`).

Quickshell colours the block with KDE's `SyntaxHighlighter` and the 463 syntax definitions
installed with the library. impulse-shell reads the same definition files with its own code
(`crates/qml/src/syntax/kate.rs`) and carries 41 of them: the 29 languages below and the 12
definitions those include (`crates/qml/src/syntax/data/LICENSES.md` lists every file).

| | Quickshell 0.3.2 | impulse-shell |
|---|---|---|
| A block of one of the 29 languages | Coloured by its definition. | The same colours, character for character. |
| A block of any other language KDE has a definition for (`perl`, `haskell`, `zsh`, ...) | Coloured by its definition. | Not coloured: all of it in the theme's Normal colour, as Quickshell shows a language KDE has no definition for. |
| Code of another language inside a `markdown` block (a fenced block) or a `latex` block (`lstlisting`, `minted` and the like) | Coloured by the definition of that language, for each of the 38 definitions Markdown's definition names and the 141 LaTeX's names. | Coloured only for the definitions in the two lists below (24 and 30). Code of the others is drawn as the surrounding definition draws text it does not parse: one colour for the whole block. |

The 29 languages (the names `Repository.definitionForName` takes): Bash, C, CMake, C++, C#,
CSS, Diff, Dockerfile, Go, HTML, Java, JavaScript, JSON, Kotlin, LaTeX, Lua, Makefile,
Markdown, Nix, Python, QML, Ruby, Rust, SQL, Swift, TOML, TypeScript, XML, YAML.

Coloured inside a `markdown` block: Bash, C, C#, C++, CMake, CSS, Comments, Doxygen, Go, HTML,
JSON, Java, JavaScript, JavaScript React (JSX), Mustache/Handlebars (HTML), Python, QML, Ruby,
Rust, TOML, TypeScript, XML, YAML, reStructuredText.

Coloured inside a `latex` block: Bash, C, C#, C++, CMake, CSS, Comments, Diff, Go, HTML, JSON,
Java, JavaScript, Kotlin, LaTeX, Lua, Makefile, Markdown, Nix, Python, QML, Ruby, Rust, SQL,
Swift, TOML, TypeScript, XML, YAML, reStructuredText.

Why: the definitions are compiled into `impulse-shell`. All that Markdown and LaTeX can name are
156 files and 5.5 MB; the 41 carried are 1.9 MB. The languages are the ones the chat's models
send.

Tests:

- `code_is_coloured_as_quickshell_colours_it` in `crates/shell/tests/code_colours.rs` draws a
  text of each of the 29 languages, and of four names KDE has no definition for, with the
  real `qs` under KDE's highlighter and with impulse-shell's colours, in both of the original's themes, and
  compares the pictures pixel for pixel. It holds the first row.
- `code_of_a_language_outside_the_set_is_coloured_only_by_quickshell` in the same file holds
  the other two rows against `qs`: the pictures differ, and only inside the code of the
  language impulse-shell does not carry.
- `nested_code_is_coloured_for_the_languages_of_the_set` in `crates/qml/src/syntax.rs` checks
  the two lists against the definitions carried.

## "Clear all" of the notification list

Compared with Quickshell 0.3.2 under sway, on three notifications and
`Notifications.discardAllNotifications` (`qml/services/Notifications.qml:254`), which the
clear button of the right sidebar calls. It empties its own list, runs
`notifServer.trackedNotifications.values.forEach((notif) => { notif.dismiss() })` and
destroys the wrapper of each notification.

| | Quickshell 0.3.2 | impulse-shell |
|---|---|---|
| When `dismiss()` takes the notification out of `trackedNotifications` | Inside the call (`NotificationServer::deleteNotification`): the list is one shorter when `dismiss()` returns. | After the script that called it. |
| Which notifications the `forEach` dismisses | The first and the third. The list shrinks under the `forEach`, which skips every second one. | All three. |
| `NotificationClosed` sent to the senders | Two of three, for ids 1 and 3. | Three of three. |
| The notification that was skipped | Stays on the server and in `trackedNotifications`, in no list on screen; its sender can still close it. | None is skipped. |
| The text of a wrapper (`summary: notification?.summary ?? ""`), which its card shows while it slides out | Empty for the two dismissed: their `Notification` is deleted while the wrapper is still there. The skipped one keeps its text. | No wrapper's text changes. |

After the click the list on screen is empty in both.

Why: "clear all" is meant to close every notification. A program that waits for
`NotificationClosed` to tidy up after its notification never gets it for every second one
from Quickshell, and each of those stays in the server's memory.

Not held by a test: the cards themselves. In frames of both shells on the whole `qml/` (the
right sidebar open, three notifications, the clear button) Quickshell's cards slide out
without their text for about 0.1 s and impulse-shell's with it.

Tests, in `crates/shell/tests/notification_clear_all_equiv.rs`, on
`notification_clear_all/shell.qml`, which is `discardAllNotifications` in small:

- `quickshell_clears_every_second_notification` runs the real `qs`: `NotificationClosed` for
  1 and 3 with reason 2, the second still there for `CloseNotification` (reason 3), and the
  wrappers of 1 and 3 with an empty summary.
- `clear_all_closes_every_notification` runs impulse-shell: `NotificationClosed` for 1, 2 and 3 with
  reason 2, nothing left to close, no wrapper with an empty summary.
