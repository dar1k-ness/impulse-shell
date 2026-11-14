# Clocks in production code

The shell waits on events: a readable fd, a child's exit (pidfd), a D-Bus signal, an inotify
change, a frame callback. A clock is used only where no event can say the thing: something
did **not** happen (a silent peer, a process that ignores SIGTERM), a rate limit, or a moment
in time that is the feature itself (an animation, a double click, 60 frames a second).

`crates/shell/tests/lifecycle_waits_on_events.rs`
(`every_production_clock_is_listed_with_its_reason_and_test`) scans `crates/*/src` for timed
waits, time limits, comparisons of a span of time with a limit (`elapsed()`, `Instant::now()`,
`duration_since(`), constants named like a duration, and lengths of time whatever they are
named: a `const` or `static` whose type holds a `Duration`, and a `Duration` built from a
number (`Duration::from_secs(45)`). A comparison split over two lines with none of those forms
on the line of the operator is not found. It fails
when one is in the code but not in a table here, when a row here matches no line of the code,
or when the test a row names does not exist. A new clock therefore needs a reason and a test
that shows it at work; where there is an event, wait on the event instead.

A row is the file, a piece of the line, why a clock is right there, and the test.

## Not answered: limits on someone else's silence

A peer that hangs sends nothing, so nothing but a clock ends the wait. Each wait ends at
once with the answer; the limit is only reached when there is none.

| File | Line has | Why a clock | Test |
| --- | --- | --- | --- |
| `crates/bus/src/lib.rs` | `Timer::after(timeout)` | `bus::call_timeout` and `bus::call_until`: a D-Bus peer that holds a call never replies. | `crates/bus/tests/call_timeout.rs` `a_call_ends_with_the_reply_or_at_the_limit` |
| `crates/bus/src/lib.rs` | `wake.wait_timeout(g, at.saturating_duration_since(now))` | `bus::call_then_within`: the connection's limiter thread sleeps until the nearest limit of the calls written without waiting for their answers. A peer that stays on the bus and never answers sends nothing; the call is then forgotten. An answer takes the call's limit away, a nearer limit and the connection closing wake the thread. | `crates/bus/tests/call_then.rs` `a_held_call_ends_at_its_limit_and_is_forgotten` |
| `crates/qml/src/notifier.rs` | `const REPLY_LIMIT` | 25 s for the notification server to answer `GetCapabilities` and `Notify` of a `Notify.send` with buttons: GDBus's default reply timeout (`-1`), which libnotify makes both calls with, so `notify-send -A` gave up on each then too (0.8.9 against a silent server: "Displaying non-interactively" and "Failed to show notification: Timeout was reached", each 25 s after its call). A server that never answers must not leave the call, and what the server says meanwhile about ids that may be its notification, kept for ever. | `crates/qml/tests/notifications_rusty.rs` `a_notify_the_server_never_answers_is_given_up_at_the_reply_limit` |
| `crates/qml/src/sni.rs` | `const MENU_REPLY_TIMEOUT` | 25 s for a tray app to answer `AboutToShow`, `GetLayout` or `GetGroupProperties`, the reply timeout Qt gives Quickshell's dbusmenu calls: an app that holds one must not hold its menu's queue. The menu closing ends the wait early. | `crates/qml/src/sni.rs` `a_silent_apps_menu_fetch_ends_at_the_reply_limit` |
| `crates/qml/src/sni.rs` | `call_until(` | The call that `MENU_REPLY_TIMEOUT` limits. | `crates/qml/src/sni.rs` `a_silent_apps_menu_fetch_ends_at_the_reply_limit` |
| `crates/colorgen/src/kde/mod.rs` | `const TIMEOUT` | 2 s per KDE D-Bus call while applying a theme: a hung KWin or Plasma must not hold the theme switch. | `crates/bus/tests/call_timeout.rs` `a_call_ends_with_the_reply_or_at_the_limit` |
| `crates/colorgen/src/kde/mod.rs` | `bus::call_timeout(` | The call that `TIMEOUT` limits. | `crates/bus/tests/call_timeout.rs` `a_call_ends_with_the_reply_or_at_the_limit` |
| `crates/qml/src/notifier.rs` | `wait_timeout_while(queued, left,` | The shell's exit waits for the `Notify.send`s that are not on the session bus yet (queued behind its opening, or with buttons and waiting for the server's `GetCapabilities` answer), until `FLUSH`, 2 s, from the start of the exit, for a bus or server that never answers; then the process ends without them. The wait ends at once when the last one is on the bus. | `crates/qml/tests/dbus_offmain.rs` `notify_flush_gives_up_on_a_silent_bus_at_its_limit` |
| `crates/qml/src/notifier.rs` | `const FLUSH` | The limit of the exit's waits for services that do not answer, 2 s for all of them together (`flush_for_exit` gives `notifier::flush_until` and `bluez::flush_until` the same end). | `crates/qml/tests/dbus_offmain.rs` `notify_flush_gives_up_on_a_silent_bus_at_its_limit` |
| `crates/qml/src/bluez.rs` | `wait_timeout_while(n, left,` | The shell's exit waits until the calls and property writes made so far are written to the bus (`StopDiscovery` from `Component.onDestruction` at `Qt.quit()`), not for bluetoothd's answer. A write queued behind one bluetoothd has not answered is waited for until `notifier::FLUSH`, 2 s, from the start of the exit; then the process ends. | `crates/qml/tests/sysbus_services.rs` `bluetooth_flush_waits_for_the_write_to_be_sent_not_answered` |
| `crates/workpool/src/lib.rs` | `wait_timeout_while(held, limit,` | `workpool::finish_held`: the shell's exit asks again for the threads the system refused and waits for the jobs that waited for them. A system that still refuses never says so again, so the wait has the caller's limit; it ends at once when those jobs have run, and there is none to wait for unless a thread was refused ([threads.md](threads.md)). | `crates/workpool/src/lib.rs` `finish_held_asks_again_and_waits_for_the_jobs_refused_their_threads` |
| `crates/shell/src/main.rs` | `const HELD` | The limit of that wait at the shell's exit, 2 s, cut to what is left of `notifier::FLUSH` from the start of the exit. | `crates/workpool/src/lib.rs` `finish_held_asks_again_and_waits_for_the_jobs_refused_their_threads` |
| `crates/qml/src/probe.rs` | `call_timeout(` | 5 s for the system bus to list its names (GeoClue probe). | `crates/bus/tests/call_timeout.rs` `a_call_ends_with_the_reply_or_at_the_limit` |
| `crates/qml/src/probe.rs` | `names(m, std::time::Duration::from_secs(5))` | The 5 s of that call, for each of the two lists. | `crates/qml/src/probe.rs` `geoclue_check_gives_each_list_five_seconds` |
| `crates/qml/src/sysbus.rs` | `const REPLY_TIMEOUT` | 25 s for a system daemon (NetworkManager, BlueZ, UPower, power-profiles-daemon, the notification daemon) or the bus daemon to answer a call, as libdbus, GDBus and QtDBus give a call that names no timeout: a daemon that keeps its name and never answers must not hold the calls queued behind it. | `crates/qml/tests/sysbus_services.rs` `silent_network_manager_scan_ends_and_the_next_action_runs` |
| `crates/qml/src/sysbus.rs` | `Duration::from_millis(REPLY_MS.load(Ordering::SeqCst))` | The limit in use: `REPLY_TIMEOUT`, or the one a test set with `sysbus::set_reply_timeout`. | `crates/qml/tests/sysbus_services.rs` `silent_network_manager_scan_ends_and_the_next_action_runs` |
| `crates/qml/src/sysbus.rs` | `call_timeout(` | The call that `REPLY_TIMEOUT` limits. | `crates/qml/tests/sysbus_services.rs` `silent_network_manager_scan_ends_and_the_next_action_runs` |
| `crates/qml/src/sysbus.rs` | `Timer::after(limit)` | `REPLY_TIMEOUT` for a request zbus makes itself (`RequestName`, `ReleaseName`), for weather's first read of NetworkManager's `State` and for the pairing agent's read of a device's name. | `crates/qml/src/sysbus.rs` `a_request_ends_with_its_answer_or_at_the_limit` |
| `crates/qml/src/position.rs` | `const UPDATE_TIMEOUT` | `PositionSource.update()`: 120 s for GeoClue to report a fix (Qt's geoclue2 plugin, `UPDATE_TIMEOUT_COLD_START`), or the timeout the call names; no fix is not an event. | `crates/qml/src/position.rs` `the_wait_ends_with_an_event_or_at_the_single_updates_deadline` |
| `crates/qml/src/position.rs` | `Duration::from_secs_f64(ms / 1000.0)` | The timeout `update(timeout)` names, in place of `UPDATE_TIMEOUT`. | `crates/qml/src/position.rs` `a_single_update_waits_the_timeout_it_names` |
| `crates/qml/src/position.rs` | `Timer::at(at)` | The deadline of that single update, waited on together with GeoClue's signals and the stop. | `crates/qml/src/position.rs` `the_wait_ends_with_an_event_or_at_the_single_updates_deadline` |
| `crates/colorgen/src/kde/pywal.rs` | `set_write_timeout` | i3/sway reload over their IPC socket: 1 s for a stopped compositor to take the command. | `crates/colorgen/src/kde/pywal.rs` `ipc_command_takes_the_answer_or_gives_up_on_a_silent_socket` |
| `crates/colorgen/src/kde/pywal.rs` | `set_read_timeout` | The same, for its answer. | `crates/colorgen/src/kde/pywal.rs` `ipc_command_takes_the_answer_or_gives_up_on_a_silent_socket` |
| `crates/colorgen/src/kde/pywal.rs` | `deadline(Duration::from_secs(2))` | `xrdb -merge` of the new colours is waited for 2 s, then killed: an `xrdb` that does not return must not hold the theme switch. Its exit ends the wait at once. | `crates/colorgen/src/kde/pywal.rs` `xrdb_is_given_two_seconds` |
| `crates/hypripc/src/lib.rs` | `set_read_timeout(read_timeout)` | The caller's limit on Hyprland's reply; the shell's own requests pass none. | `crates/qml/src/hyprctl.rs` `hypr_unanswered_request_times_out_like_hyprctl` |
| `crates/qml/src/hyprctl.rs` | `const RCVTIMEO` | hyprctl's own 5 s `SO_RCVTIMEO`, kept so scripts see the same error from a wedged Hyprland. | `crates/qml/src/hyprctl.rs` `hypr_unanswered_request_times_out_like_hyprctl` |
| `crates/qml/src/http.rs` | `const STALL` | 30 s of silence from an HTTP server fails a request that asked for it (images, wallpapers); curl waits forever. | `crates/shell/tests/remote_image_limits.rs` `stalled_server_fails_the_image` |
| `crates/qml/src/http.rs` | `set_read_timeout(self.stall)` | `STALL` on the socket's reads. | `crates/shell/tests/remote_image_limits.rs` `stalled_server_fails_the_image` |
| `crates/qml/src/http.rs` | `set_write_timeout(self.stall)` | `STALL` on the socket's writes. | `crates/shell/tests/remote_image_limits.rs` `stalled_server_fails_the_image` |
| `crates/qml/src/http.rs` | `Timer::after(t)` | The caller's limit on a TCP handshake: a swallowed SYN gets no answer. | `crates/qml/src/http/tests.rs` `a_connect_ends_with_the_handshake_or_at_the_limit` |
| `crates/qml/src/http.rs` | `const HAPPY_EYEBALLS` | RFC 8305, curl's default: the next address is tried 200 ms after the last, or at once when it fails. | `crates/qml/src/http/tests.rs` `happy_connect_skips_blackholed_family` |
| `crates/qml/src/http.rs` | `Timer::after(delay)` | The 200 ms above. | `crates/qml/src/http/tests.rs` `happy_connect_skips_blackholed_family` |
| `crates/qml/src/http.rs` | `const KEEPIDLE` | TCP keepalive: the kernel notices a vanished peer in 30 s; a live, slow server is never cut off. | `crates/qml/src/http/tests.rs` `keepalive_gives_up_on_a_silent_peer_in_30s` |
| `crates/qml/src/http.rs` | `timeout_recv_response(answer_within)` | The caller's limit on a reply (`Opts::answer_within`): a silent server sends no event. Only the translator sets it, translate-shell's rule: 2 s without a response, then once more over IPv4 with no limit. | `crates/qml/src/jobs_wall_gcloud_trans/trans/tests.rs` `a_silent_server_is_given_up_and_asked_again` |
| `crates/qml/src/http.rs` | `timeout_recv_body(answer_within)` | The same, for the body. | `crates/qml/src/jobs_wall_gcloud_trans/trans/tests.rs` `a_silent_server_is_given_up_and_asked_again` |
| `crates/qml/src/http.rs` | `set_read_timeout(limit)` | The same limit on the socket's read, when it is shorter than `STALL`. | `crates/qml/src/jobs_wall_gcloud_trans/trans/tests.rs` `a_silent_server_is_given_up_and_asked_again` |
| `crates/qml/src/jobs_wall_gcloud_trans/trans.rs` | `answer_within: Some(std::time::Duration::from_secs(2))` | The translator's 2 s for that limit, on its first request only. | `crates/qml/src/jobs_wall_gcloud_trans/trans/tests.rs` `a_silent_server_is_given_up_and_asked_again` |
| `crates/qml/src/ports/songrec.rs` | `libc::poll(&mut p, 1, ms)` | Song recognition listens until a match or the caller's deadline; no match is not an event. | `crates/qml/src/ports/songrec.rs` `first_match_skips_other_lines_and_stops_at_eof_or_the_deadline` |
| `crates/qml/src/ports/songrec.rs` | `Instant::now() >= d` | The same deadline, checked when the poll returns. | `crates/qml/src/ports/songrec.rs` `first_match_skips_other_lines_and_stops_at_eof_or_the_deadline` |
| `crates/qml/src/ports/songrec.rs` | `Duration::try_from_secs_f64(secs)` | The caller's `-t` seconds, turned into that deadline; a number no clock can hold is no deadline. | `crates/qml/src/ports/songrec.rs` `a_total_no_clock_can_hold_is_no_deadline` |
| `crates/qml/src/ports/wl.rs` | `libc::poll(fds.as_mut_ptr(), 2, ms)` | A screen capture waits for the compositor's frame until `ports::CAPTURE_LIMIT` (2 s): a compositor that takes the request and never answers sends no event, and the window that waits for the capture would get no frames. The job's stop ends the wait early. | `crates/shell/src/main.rs` `a_capture_the_compositor_never_answers_releases_its_window` |
| `crates/qml/src/ports/mod.rs` | `const CAPTURE_LIMIT` | The limit of that wait. | `crates/shell/src/main.rs` `a_capture_the_compositor_never_answers_releases_its_window` |
| `crates/qml/src/watchdog.rs` | `const DEFAULT_TIMEOUT` | A script that never returns: 5 s of the thread's CPU time, then it is interrupted. The watchdog sleeps until the timer armed for an evaluation expires. | `crates/qml/src/watchdog.rs` `only_an_evaluation_that_alone_runs_out_of_time_is_interrupted` |
| `crates/qml/src/watchdog.rs` | `let d = Duration::from_millis(ms);` | Arms the thread's monotonic timer with that limit (or the one `watchdog::set_timeout` set) for the running evaluation. Not a timer on the thread's CPU clock: at the expiry of one the kernel kills a process whose realtime budget (`RLIMIT_RTTIME`) is 0. | `crates/qml/src/watchdog.rs` `only_an_evaluation_that_alone_runs_out_of_time_is_interrupted` |
| `crates/qml/src/watchdog.rs` | `let out_of_time = Duration::from_nanos(until);` | The CPU time the evaluation has left when the timer expires: the timer is armed again for it, so the limit counts the thread's CPU time, not wall time. | `crates/qml/src/watchdog.rs` `timeout_counts_the_threads_cpu_time_not_wall_time` |
| `crates/qml/src/watchdog.rs` | `const SHORTEST_REST` | The shortest time that timer is armed for again, 1 ms: an evaluation a moment short of its CPU time on a thread that waits for a core does not keep the watchdog spinning. | `crates/qml/src/watchdog.rs` `timeout_counts_the_threads_cpu_time_not_wall_time` |
| `crates/qml/src/pam.rs` | `const HELPER_WAIT` | A password check that just started (`PamContext.start()`): the frames of the windows that are not lock surfaces wait for the turn that reads the helper's first message, which the lock screen answers with the password; a frame drawn in between (a blurred wallpaper takes 0.8 s without a GPU) starts the check that much later. That message ends the wait at once. A helper that sends none (a module waiting for a finger on a reader) says nothing, so the frames are held for 100 ms at most. | `crates/qml/src/pam.rs` `a_started_helper_is_waited_for_until_its_first_event` |
| `crates/wallpick/src/http.rs` | `const LOOKUP` | The wallpaper picker's own HTTP (ureq, on the pick's worker thread): 15 s for the resolver to answer for a wallpaper site's name; a resolver that hangs sends nothing. | `crates/wallpick/src/http.rs` `a_name_nobody_resolves_is_given_up_at_the_limit` |
| `crates/wallpick/src/http.rs` | `const CONNECT` | 15 s for the TCP handshake with it: a swallowed SYN gets no answer. | `crates/wallpick/src/http.rs` `a_handshake_nobody_answers_is_given_up_at_the_limit` |
| `crates/wallpick/src/http.rs` | `timeout_connect(Some(limits.connect))` | `CONNECT` given to ureq. | `crates/wallpick/src/http.rs` `a_handshake_nobody_answers_is_given_up_at_the_limit` |
| `crates/wallpick/src/http.rs` | `const ANSWER` | 30 s for the response's head: a site that took the request and says nothing must not hold the pick. | `crates/wallpick/src/http.rs` `a_server_that_never_answers_is_given_up_at_the_limit` |
| `crates/wallpick/src/http.rs` | `timeout_recv_response(Some(limits.answer))` | `ANSWER` given to ureq. | `crates/wallpick/src/http.rs` `a_server_that_never_answers_is_given_up_at_the_limit` |
| `crates/wallpick/src/http.rs` | `const BODY_STALL` | 30 s of silence in the middle of a body (a thumbnail, a wallpaper, a model file) fails the download instead of hanging the worker; each read that brings bytes starts it again, so a slow transfer is never cut off. | `crates/wallpick/src/http.rs` `a_body_that_stalls_fails_the_download` |

## Grace and give-up deadlines

| File | Line has | Why a clock | Test |
| --- | --- | --- | --- |
| `crates/qml/src/childguard.rs` | `const GRACE` | SIGTERM, then SIGKILL after 2 s for a process group that ignores it. The group's exit ends the wait early. | `crates/qml/src/childguard/tests.rs` `a_group_ignoring_sigterm_is_killed_after_the_grace` |
| `crates/qml/src/childguard.rs` | `timerfd_settime` | The timerfd that carries `GRACE`, polled together with the group's pidfds. | `crates/qml/src/childguard/tests.rs` `a_group_ignoring_sigterm_is_killed_after_the_grace` |
| `crates/colorgen/src/live.rs` | `const KILL_DEADLINE` | Waiting for a killed process to be gone (pidfd) gives up after 2 s on one that does not die. | `crates/colorgen/tests/kill_exact.rs` `gives_up_while_the_target_runs` |
| `crates/colorgen/src/live.rs` | `timerfd_settime` | The timerfd that carries `KILL_DEADLINE` and `TTY_STALL`. | `crates/colorgen/tests/kill_exact.rs` `gives_up_while_the_target_runs` |
| `crates/colorgen/src/live.rs` | `after.max(Duration::from_nanos(1))` | A limit of zero still arms that timerfd, for 1 ns: an all-zero value would disarm it and the wait would never give up. | `crates/colorgen/tests/kill_exact.rs` `gives_up_while_the_target_runs` |
| `crates/colorgen/src/live.rs` | `const TTY_STALL` | Terminal colours are written whole: after `EAGAIN` the write waits for the pts to be writable (`POLLOUT`). A terminal that takes nothing for 500 ms (its buffer is full and nobody reads it, or its output is stopped) must not hold the writing of colours: the rest is left. | `crates/colorgen/tests/tty_write.rs` `an_output_that_takes_nothing_does_not_hold_the_writer` |
| `crates/colorgen/src/kde/kconfig.rs` | `timerfd_settime` | The deadline a wait is given as an fd: a KDE config lock is waited for until `LOCK_TIMEOUT`, a reload child until its limit, not forever. | `crates/colorgen/src/kde/kconfig.rs` `gives_up_on_a_live_lock_at_the_deadline` |
| `crates/colorgen/src/kde/kconfig.rs` | `const LOCK_TIMEOUT` | KConfig's rule (`KConfigIniBackend::lock`): 45 s for a config file's lock, then the write is given up and the file stays as it is. | `crates/colorgen/src/kde/kconfig.rs` `a_file_whose_lock_cannot_be_had_is_not_written` |
| `crates/colorgen/src/kde/kconfig.rs` | `const STALE_AFTER` | QLockFile's rule: a lock file 30 s old that nobody holds a `flock` on is nobody's. A file reaching an age is not an event. | `crates/colorgen/src/kde/kconfig.rs` `takes_over_an_unlocked_lock_when_it_turns_stale` |
| `crates/colorgen/src/kde/kconfig.rs` | `libc::poll(` | One sleep until the lock is removed (inotify), its owner exits (pidfd), the deadline fires (timerfd) or the lock turns stale (the timeout, the time left to `STALE_AFTER`). | `crates/colorgen/src/kde/kconfig.rs` `waits_for_the_age_of_another_machines_lock` |
| `crates/colorgen/src/kde/kconfig.rs` | `Duration::from_millis((stale_at - a + 1) as u64)` | The timeout of that sleep: the time left until the lock file is `STALE_AFTER` old. | `crates/colorgen/src/kde/kconfig.rs` `waits_for_the_age_of_another_machines_lock` |
| `crates/colorgen/src/kde/kconfig.rs` | `limit.max(Duration::from_nanos(1))` | A limit of zero still arms that timerfd, for 1 ns: an all-zero value would disarm it. | `crates/colorgen/src/kde/kconfig.rs` `a_deadline_of_zero_fires` |
| `crates/shell/src/reload.rs` | `const PREDECESSOR_STAYS` | After a reload the old image's windows stay on the screen until every window of the new image has its first frame and none of its pictures is still loading or fading in (the wallpaper's window is empty until then). A window the compositor never configures or never takes a frame of sends no event, and a picture's load may never end, so the old windows are given up 5 s after the new image's first turn of its loop (`reload::predecessor_leaves`, asked on each turn). The first frames and the pictures end the wait early. | `crates/shell/src/reload.rs` `the_previous_images_windows_leave_at_the_first_frames_or_past_the_limit` |

## Work that is due at a time

| File | Line has | Why a clock | Test |
| --- | --- | --- | --- |
| `crates/shell/src/gpu.rs` | `const LAYER_IDLE_TIME` | A cached GPU layer left undrawn for 60 s is freed. An expiry, not a poll: a shell at rest draws a frame every few seconds, so frames drawn do not say how long a closed panel's layers have sat. Checked in the loop turns there are; no timer runs and nothing is woken for it. | `crates/shell/src/gpu.rs` `layers_left_undrawn_are_freed_by_the_clock` |
| `crates/shell/src/gpu.rs` | `const STAMP_EVERY` | How finely that expiry tells frames apart: the time of at most two frames a second is kept. Nothing waits on it. | `crates/shell/src/gpu.rs` `layers_left_undrawn_are_freed_by_the_clock` |
| `crates/shell/src/gpu.rs` | `saturating_duration_since(s.1) >= LAYER_IDLE_TIME` | Where `LAYER_IDLE_TIME` is applied: which of the frames whose time is kept is the newest drawn 60 s ago or earlier, asked when a frame is drawn and when the idle layers are freed. Two readings compared; nothing sleeps. | `crates/shell/src/gpu.rs` `layers_left_undrawn_are_freed_by_the_clock` |
| `crates/shell/src/gpu.rs` | `saturating_duration_since(self.stamps[n - 2].1) < STAMP_EVERY` | Where `STAMP_EVERY` is applied: a frame drawn less than a second after the last but one takes the last kept time instead of adding one. Nothing waits on it. | `crates/shell/src/gpu.rs` `layers_left_undrawn_are_freed_by_the_clock` |
| `crates/workpool/src/lib.rs` | `wait_timeout(g, at - now)` | A delayed job: the worker sleeps until the earliest one is due or a new job arrives. | `crates/workpool/src/lib.rs` `delayed_job_waits_without_blocking_others` |
| `crates/workpool/src/lib.rs` | `const LONG_IDLE` | A thread for long jobs exits after 10 s without one: threads are reused in a burst and cost nothing at rest. | `crates/workpool/src/lib.rs` `long_threads_are_reused_then_exit_when_idle` |
| `crates/workpool/src/lib.rs` | `wait_timeout(g, self.idle - idled)` | The wait that `LONG_IDLE` ends. | `crates/workpool/src/lib.rs` `long_threads_are_reused_then_exit_when_idle` |
| `crates/imgtools/src/pool.rs` | `const IDLE` | The image worker threads exit after 3 s without work, for the same reason. | `crates/imgtools/tests/pool.rs` `pool_is_shared_by_a_burst_and_exits_when_idle` |
| `crates/imgtools/src/pool.rs` | `wait_timeout(s, IDLE - e)` | The wait that `IDLE` ends. | `crates/imgtools/tests/pool.rs` `pool_is_shared_by_a_burst_and_exits_when_idle` |
| `crates/qml/src/brightness.rs` | `const SET_VCP_DELAY` | DDC/CI requires this pause after a Set VCP Feature; the monitor sends nothing when it is ready. | `crates/qml/src/brightness.rs` `ddc_writes_are_read_back_once_after_they_stop` |
| `crates/qml/src/brightness.rs` | `recv_timeout(` | The DDC thread waits for the next write or for that pause to end, then reads the value back once. | `crates/qml/src/brightness.rs` `ddc_writes_are_read_back_once_after_they_stop` |
| `crates/qml/src/jobs_wall_gcloud_trans/gcloud.rs` | `stop.wait_timeout(d)` | gcloud's own retry backoff for a failed API call; a stop ends it at once. | `crates/qml/src/jobs_wall_gcloud_trans/gcloud/tests.rs` `a_stop_during_the_backoff_ends_the_retries` |
| `crates/qml/src/jobs_wall_gcloud_trans/gcloud.rs` | `Duration::from_secs_f64(uniform(wait * 0.9, wait * 1.1))` | The length of that backoff, as google-auth's `ExponentialBackoff`: about 1 s, then 2 s, each within 10 %. | `crates/qml/src/jobs_wall_gcloud_trans/gcloud/tests.rs` `a_stop_during_the_backoff_ends_the_retries` |
| `crates/qml/src/svc.rs` | `fn wait_timeout(` | `Stop::wait_timeout`: the stop-aware wait the backoff above uses. | `crates/qml/src/jobs_wall_gcloud_trans/gcloud/tests.rs` `a_stop_during_the_backoff_ends_the_retries` |
| `crates/qml/src/svc.rs` | `wait_timeout_while(` | The condvar wait inside it. | `crates/qml/src/jobs_wall_gcloud_trans/gcloud/tests.rs` `a_stop_during_the_backoff_ends_the_retries` |
| `crates/qml/src/mpris.rs` | `Duration::from_millis(100)` | Quickshell's rule (`MprisPlayer::onPlaybackStatusUpdated`, 0.3.2): each time a player announces its PlaybackStatus its Position is read at once and again 100 ms later. MPRIS has no change signal for Position, and some players (YouTube in a browser) set it at an indeterminate time after they announce the status, with no Seeked signal: nothing says when, so the second read is the announcement plus that time. With no announcement nothing is scheduled. | `crates/qml/tests/bus_events.rs` `mpris_position_read_again_when_the_status_is_announced` |
| `crates/qml/src/mpris.rs` | `Timer::after(d)` | The wait that carries those 100 ms, one per announcement; a player gone meanwhile is not read. | `crates/qml/tests/bus_events.rs` `mpris_position_read_again_when_the_status_is_announced` |
| `crates/qml/src/layout_model/store.rs` | `const MTIME_GRANULARITY` | A file written within 2 s of its mtime may change again without the mtime moving (coarse filesystem timestamps), so it is read again. | `crates/qml/src/layout_model/store.rs` `a_racy_same_size_rewrite_is_still_seen` |
| `crates/shell/src/main.rs` | `Duration::from_secs_f64(c.ms / 1000.0)` | The colour picker after a panel Hyprland animates away: Hyprland says when the close animation of a layer begins (`closelayer` on its event socket) and never when it ends, so the end is the beginning plus the animation's length from Hyprland's own settings, with nothing added. The first frame shown after it starts hyprpicker. | `crates/shell/tests/color_picker_off_screen.rs` `hyprpicker_starts_after_hyprlands_close_animation` |
| `crates/shell/src/main.rs` | `Duration::from_secs_f64(close.ms / 1000.0)` | The same length for a panel closed after the event socket was lost: nothing says when its animation began, and it has by the time the shell learns the panel closed. | `crates/shell/tests/color_picker_off_screen.rs` `hyprpicker_starts_when_hyprlands_event_socket_is_lost` |

## Rate limits

The event is real (a destroyed object, a changed file, a new client); the clock only says
how often the work it triggers may run. With no event pending nothing is scheduled.

| File | Line has | Why a clock | Test |
| --- | --- | --- | --- |
| `crates/qml/src/runtime.rs` | `const REAP_EVERY` | Destroyed objects are swept at most every 250 ms, or at once when 512 are waiting: one sweep walks whole tables, and an animation may destroy objects on every frame without end. | `crates/qml/src/runtime.rs` `deaths_are_swept_in_batches` |
| `crates/qml/src/runtime.rs` | `elapsed() >= REAP_EVERY` | The check of the row above. | `crates/qml/src/runtime.rs` `deaths_are_swept_in_batches` |
| `crates/shell/src/sched.rs` | `const PURGE_EVERY` | Memory is handed back to the OS at most every 2 s, and only after 200 deaths or 50 000 new objects, never during an animation. | `crates/shell/src/sched.rs` `purges_need_work_and_are_spaced` |
| `crates/shell/src/sched.rs` | `const STALE_RELAYOUT_MS` | A hidden window whose content changed is laid out again once it has been left alone for 300 ms, not per change. | `crates/shell/src/sched.rs` `a_stale_hidden_window_waits_until_it_is_left_alone` |
| `crates/qml/src/desktop.rs` | `const DEBOUNCE_MS` | Quickshell's rule: applications are rescanned 100 ms after the last change in their directories (a package install touches many files). | `crates/qml/tests/desktopwatch.rs` `matches_quickshell` |
| `crates/shell/src/ipc.rs` | `const IDLE` | Checked only when a client connects while all 64 slots are taken: a connection silent for 30 s gives its slot up. No timer runs. | `crates/shell/src/ipc.rs` `an_idle_connection_gives_way_at_the_cap` |
| `crates/shell/src/ipc.rs` | `c.last.elapsed() >= IDLE` | The check of the row above. | `crates/shell/src/ipc.rs` `an_idle_connection_gives_way_at_the_cap` |
| `crates/qml/src/hypr.rs` | `st.wait_timeout(d)` | The pause before reconnecting to Hyprland's event socket when it closed soon after connecting: 100 ms, doubled on each such close up to 5 s; a stop ends it at once. Hyprland closes a client it cannot write to and accepts it again at once, and reconnecting without a pause spun a core. | `crates/qml/src/hypr.rs` `reconnects_to_a_peer_that_closes_at_once_after_a_pause` |
| `crates/qml/src/hypr.rs` | `since.elapsed() >= RECONNECT_MAX` | The check that picks that pause: a connection that lasted 5 s is reconnected without one. | `crates/qml/src/hypr.rs` `reconnects_to_a_peer_that_closes_at_once_after_a_pause` |
| `crates/qml/src/hypr.rs` | `const RECONNECT_MIN` | The first pause before reconnecting, 100 ms. | `crates/qml/src/hypr.rs` `reconnects_to_a_peer_that_closes_at_once_after_a_pause` |
| `crates/qml/src/hypr.rs` | `const RECONNECT_MAX` | The longest pause before reconnecting, and how long a connection must last to be reconnected without one: 5 s. | `crates/qml/src/hypr.rs` `the_pause_before_reconnecting_doubles_up_to_its_longest` |

## Time is the feature

Qt's own timing of input and drawing. QML `Timer`s, animations, key repeat and the caret
blink are the same kind and are not listed line by line: each pushes its next due time into
the main loop's one deadline queue (`crates/shell/src/sched.rs`), the loop sleeps until the
earliest is due or an event arrives, and with nothing due it blocks without a timeout
(`crates/shell/tests/idle_wakeups.rs`).

| File | Line has | Why a clock | Test |
| --- | --- | --- | --- |
| `crates/shell/src/input.rs` | `const DOUBLE_CLICK_MS` | Qt's double-click interval: two clicks within 400 ms are a double click. | `crates/shell/src/input_tests.rs` `mouse_area_double_click_replaces_second_clicked` |
| `crates/shell/src/textmouse.rs` | `const DOUBLE_CLICK_MS` | The same in text fields, from event timestamps. | `crates/shell/src/textmouse.rs` `click_intervals_use_event_time_not_dispatch_time` |
| `crates/shell/src/textmouse.rs` | `const PRESS_AND_HOLD_MS` | Qt's press-and-hold interval. | `crates/shell/src/textmouse.rs` `controls_hold_the_press_like_qquickpresshandler` |
| `crates/shell/src/input.rs` | `1000.0 < ms` | `WheelHandler.activeTimeout`: a scroll ends when no wheel event came for that long. | `crates/shell/src/input_tests.rs` `wheel_handler_goes_inactive_when_its_timeout_runs_out` |
| `crates/shell/src/input.rs` | `std::time::Duration::from_millis(every)` | Qt's `frameSynchronousHoverInterval`, 100 ms unless `QT_QUICK_FRAME_SYNCHRONOUS_HOVER_INTERVAL`: items that move under a still pointer get its hover again at most that often. A frame that comes sooner leaves it to a deadline of the main loop at the end of the interval; nothing is polled. | `crates/shell/src/input_tests.rs` `hover_under_a_still_pointer_comes_once_in_its_interval` |
| `crates/shell/src/input.rs` | `saturating_duration_since(timer) < every` | Where that interval is applied: a frame of the pointer's window asks whether the interval that began with the window's first frame or its last delivered hover is over. Over: the hover is delivered with the frame. Not over: the delay timer is set for its end. Two instants compared; nothing sleeps. | `crates/shell/src/input_tests.rs` `hover_under_a_still_pointer_comes_once_in_its_interval` |
| `crates/shell/src/input.rs` | `< std::time::Duration::from_micros(100)` | Qt's `QWaylandEventCompression`: a pointer frame that arrives within 100 µs of the previous one was queued behind a busy main thread, and its scroll is added to that one. Compared when the frame arrives; nothing waits. | `crates/shell/src/input.rs` `queued_axis_frames_merge_like_qt_wayland` |
| `crates/shell/src/scene.rs` | `1000.0 < DUR` | The Material placeholder floats for 300 ms when focus changes. | `crates/shell/src/scene.rs` `the_material_placeholder_floats_over_time_only_when_focus_changes` |
| `crates/qml/src/runtime.rs` | `const ICON_FADE_MS` | Kirigami's `Units.longDuration`: a `Kirigami.Icon` with `animated: true` fades from its old image into the new one over 200 ms, stepped by the animation clock like every animation. | `crates/shell/tests/kirigami_icon_fade_equiv.rs` `our_fade_moves_in_the_second_frame_after_the_write` |
| `crates/shell/src/sched.rs` | `const FRAME_CALLBACK_TIMEOUT_MS` | A compositor that does not show a window withholds its frame callback without saying so (output powered off, surface covered). After 100 ms, Qt's wait for it, the window's animations stop waiting: those that end step on the vsync timer, endless ones rest. | `crates/shell/tests/frame_callback_wait.rs` `a_transition_ends_and_timers_run_with_the_output_off` |
| `crates/shell/src/main.rs` | `>= shell::sched::FRAME_CALLBACK_TIMEOUT_MS` | The check of the row above. | `crates/shell/src/sched.rs` `a_withheld_frame_callback_is_given_up_after_its_timeout` |
| `crates/shell/src/main.rs` | `>= self.clock.vsync` | A hidden window gets no frame callbacks, so its animation steps once per refresh interval instead. So does a step whose frame changed no pixel: it is not sent ([qt-differences.md](qt-differences.md)), so no frame callback follows it. | `crates/shell/src/sched.rs` `hidden_animation_steps_on_a_vsync_timer` |
| `crates/shell/src/sched.rs` | `Duration::from_secs_f64(((at - now) / 1000.0).max(0.0))` | The main loop's one sleep: the time until the earliest entry of the deadline queue is due. With nothing due there is no timeout. | `crates/shell/src/sched.rs` `deadlines_are_ordered_by_due_time` |
| `crates/qml/src/http.rs` | `const UNUSED` | Qt's QNetworkAccessCache keeps a connection that no request uses for 120 s; `XMLHttpRequest`'s next request to the host then starts a new one, whose sockets know no login (the engine still finds it for a challenge). Compared when a request is sent; nothing waits. | `crates/qml/src/http.rs` `a_connection_unused_for_two_minutes_starts_anew` |
| `crates/qml/src/cava.rs` | `const FRAMERATE` | The audio visualizer publishes 60 frames a second, like cava, and only while a visualizer runs and an output that is on can show it: not in a locked session, nor with every output powered off (`crates/shell/tests/visualizer_off_screen.rs`). | `crates/qml/tests/cava_reconnect.rs` `follows_a_socket_activated_daemon_restart` |
| `crates/qml/src/cava.rs` | `update_timer(` | The PipeWire loop timer that carries `FRAMERATE`. | `crates/qml/tests/cava_reconnect.rs` `follows_a_socket_activated_daemon_restart` |
| `crates/qml/src/cava.rs` | `Duration::from_nanos(1_000_000_000 / FRAMERATE as u64)` | The period of that timer: a second divided by `FRAMERATE`. | `crates/qml/src/cava.rs` `a_second_holds_as_many_frames_as_the_framerate` |

## Not waits

Lines the scan finds that wait for nothing: numbers of something else, time measured only
to be logged, and lengths of time and moments that are only kept, converted or drawn.

| File | Line has | What it is |
| --- | --- | --- |
| `crates/qml/src/builtins.rs` | `const MS_PER_DAY` | JavaScript `Date` arithmetic. |
| `crates/qml/src/builtins.rs` | `const CHRONO_MS` | The range of times chrono can represent. |
| `crates/qml/src/builtins.rs` | `const CYCLE_MS` | The 400 years after which the Gregorian calendar repeats, in `Date` arithmetic. |
| `crates/shell/src/ricli.rs` | `const DAY_MS` | Julian-day arithmetic. |
| `crates/shell/src/gpu.rs` | `const LAYER_IDLE:` | Counted in frames drawn, not time. |
| `crates/shell/src/gpu.rs` | `const STALE_SWEEP` | Counted in frames drawn. |
| `crates/shell/src/gpu.rs` | `const EGL_TIMEOUT_EXPIRED_KHR` | An EGL result code; the sync is asked with a zero timeout. |
| `crates/shell/src/scene.rs` | `const FONTS_IDLE` | Counted in font lookups. |
| `crates/qml/src/native.rs` | `nfds_t, timeout)` | The main loop's poll: `-1` (until an event) or `0` (do not wait). |
| `crates/qml/src/jobs_wall_gcloud_trans/wall.rs` | `const STALL` | Request options that turn `http::STALL` on, not a duration. |
| `crates/qml/src/runtime.rs` | `as_millis() >= 5` | Time measured only to be logged (`RI_LOG` debug output). |
| `crates/shell/src/gpu.rs` | `as_micros() > 150` | Time measured only to be logged (`RI_LOG` debug output). |
| `crates/shell/src/main.rs` | `as_micros() > 300` | Time measured only to be logged (`RI_LOG` debug output). |
| `crates/shell/src/main.rs` | `as_millis() > 5` | Time measured only to be logged (`RI_LOG` debug output). |
| `crates/shell/src/main.rs` | `as_secs_f64() >= 1.0` | Debug totals (`ri::stats`), logged at most once a second while frames are drawn. |
| `crates/shell/src/scene.rs` | `as_micros() > 300` | Time measured only to be logged (`RI_LOG` debug output). |
| `crates/shell/src/gpu.rs` | `duration_since(t0).as_secs_f32() >= 1.0` | Debug totals (`ri::glcount`), logged at most once a second while something is drawn. |
| `crates/shell/src/scene.rs` | `>= PPROF_US` | `ri::pprof`: a paint slower than the threshold is logged. |
| `crates/qml/src/eval.rs` | `now - last > Duration::from_secs(5)` | `ri::jsprof`: the profile of script functions is logged at most once in 5 s, when a function returns (debug output). |
| `crates/qml/src/native.rs` | `static CROSSFADE` | The count and the length of the last `Crossfade.request(ms)`; the windows read it when they draw. |
| `crates/qml/src/native.rs` | `Duration::from_millis(ms.clamp(50, 2000))` | That length, kept between 50 ms and 2 s: how long the fade is drawn, an animation. |
| `crates/qml/src/watchdog.rs` | `Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32)` | A reading of the thread's CPU clock (`thread_cpu_time`), as a `Duration`. |
| `crates/shell/src/ipc.rs` | `static INSTANCE` | The instance's id and the time it was launched, since the epoch. |
| `crates/shell/src/ipc.rs` | `Duration::from_millis(ms)` | That launch time, handed by `Rustshell.reload()` to the image it executes, which goes on as the same instance. |
| `crates/shell/src/main.rs` | `Duration::from_secs_f64(rt.world.borrow().now.max(0.0) / 1000.0)` | The animation clock's time as an `Instant`, for the renderer. |
| `crates/shell/src/scene.rs` | `checked_sub(std::time::Duration::from_secs(1))` | A start time a second ago: a Material placeholder whose focus did not change is drawn as already floated (the 300 ms above). |
