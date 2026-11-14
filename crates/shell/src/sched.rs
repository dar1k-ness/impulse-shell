//! Main-loop wakeups as a deadline-ordered queue, the way Qt's event dispatcher keeps its
//! timers (QTimerInfoList: sorted by due time, the head bounds the poll timeout). Every source
//! of timed work (QML Timers and clocks, animation frames, key repeat, caret blink, holds)
//! pushes its next due time each turn; the loop sleeps in calloop until the head is due or an
//! event arrives, so an idle shell with nothing due blocks without a timeout.

use std::time::Duration;

/// Why the loop wakes at a deadline (for `ri::stats` and tests).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Due {
    /// Work queued for the next turn (self IPC, a frame callback while animating, motion).
    Now,
    /// World writes since the last service poll: the services that mirror them run.
    Poll,
    /// QML Timers, SystemClock, `Qt.callLater`, lock retries, desktop rescans.
    Timers,
    /// The next animation step when no window waits for a frame callback (QUnifiedTimer's
    /// timer-driven fallback when nothing is rendered).
    Frame,
    /// A changed window whose last frame was committed without a frame callback (a static
    /// frame) may draw again one refresh interval after it.
    Throttle,
    KeyRepeat,
    Caret,
    /// MouseArea press-and-hold, wheel ends, hover due again under a still pointer.
    Pointer,
    Screencopy,
    /// Hidden windows laid out while idle.
    Layout,
    Reap,
    /// Hyprland has finished animating a closed layer away (`shell::hyprclose`).
    OffScreen,
    /// The cycle collector's sweep continues at the next turn (`qml::eval::collect_step`).
    Gc,
}

/// Due times in loop milliseconds (since the loop's start), earliest first.
#[derive(Clone, Debug, Default)]
pub struct Deadlines {
    q: Vec<(f64, Due)>,
}

impl Deadlines {
    /// Adds a deadline at `at` ms; a non-finite `at` (nothing pending) is ignored.
    pub fn push(&mut self, at: f64, why: Due) {
        if !at.is_finite() {
            return;
        }
        let k = self.q.partition_point(|&(t, _)| t <= at);
        self.q.insert(k, (at, why));
    }

    pub fn next(&self) -> Option<(f64, Due)> {
        self.q.first().copied()
    }

    /// The earliest due time; `INFINITY` when nothing is pending.
    pub fn head(&self) -> f64 {
        self.next().map_or(f64::INFINITY, |(t, _)| t)
    }

    /// The calloop dispatch timeout at `now`: `None` blocks until an event.
    pub fn timeout(&self, now: f64) -> Option<Duration> {
        let (at, _) = self.next()?;
        Some(Duration::from_secs_f64(((at - now) / 1000.0).max(0.0)))
    }

    pub fn iter(&self) -> impl Iterator<Item = (f64, Due)> + '_ {
        self.q.iter().copied()
    }
}

/// How long a window waits for the frame callback of its last frame before the compositor
/// counts as not showing it (an output powered off, a surface under a fullscreen window):
/// QWaylandWindow's frame callback timeout (`QT_WAYLAND_FRAME_CALLBACK_TIMEOUT`, 100 ms),
/// after which Qt reports the window as not exposed and steps animations on a timer.
///
/// A clock by design: the event (the frame callback) is the one a compositor withholds
/// without saying so, and an animation that ends must end without it.
pub const FRAME_CALLBACK_TIMEOUT_MS: f64 = 100.0;

/// When the next animation frame needs the loop, at `now`. While every window showing an
/// animation awaits its frame callback (`awaiting`) the first callback back wakes the loop
/// and is the step; the only deadline is `give_up`, when the first of them has waited
/// [`FRAME_CALLBACK_TIMEOUT_MS`] and is waited for no longer (`None`: an endless animation of
/// a window not shown rests until its callback). Otherwise (an animating window can draw, was
/// given up on, or the animation is shown in no window) the driver steps on a repeating vsync
/// timer kept on its grid: `timer` is its last deadline. Ticks that came due while the turn
/// ran are due at once, one a turn and three at most (the answer is then at or before `now`):
/// Qt computes a step two frames ahead of the screen, and after a step that outlasted them it
/// takes the step that waited and the two it is behind by without waiting for a frame. The
/// grid goes on from the last of them. A new timer fires one vsync after the last frame
/// (`last_frame`), or after `now`.
pub fn frame_deadline(
    now: f64,
    animating: bool,
    awaiting: bool,
    give_up: Option<f64>,
    timer: Option<f64>,
    last_frame: Option<f64>,
    vsync: f64,
) -> Option<(f64, Due)> {
    if !animating {
        return None;
    }
    if awaiting {
        return give_up.map(|at| (at, Due::Frame));
    }
    let at = match timer {
        Some(t) if t > now => t,
        Some(t) => (t + vsync).max(now - 2.0 * vsync),
        None => last_frame.map_or(now + vsync, |w| (w + vsync).max(now)),
    };
    Some((at, Due::Frame))
}

/// A hidden window whose layout went stale is laid out again once it was left alone this long.
///
/// A clock by design: a debounce, not a poll. The events are the change that made the layout
/// stale and the loop going idle; laying out at every idle turn would cost one pass per
/// change of a burst (a model filling row by row) for a window nobody sees. Each change
/// restarts the wait, and nothing is scheduled while no hidden window is stale.
pub const STALE_RELAYOUT_MS: f64 = 300.0;

/// Whether a hidden window whose layout last went stale at `stale` is laid out again at `now`
/// (loop milliseconds).
pub fn stale_relayout_due(stale: f64, now: f64) -> bool {
    now - stale >= STALE_RELAYOUT_MS
}

/// Table purges and heap trims run at most this often.
///
/// A clock by design: a rate limit, not a poll. The events are destroyed instances and
/// allocated JS objects, counted since the last purge; a purge blocks the loop while it walks
/// every table, so a steady stream of them (a list scrolled for a minute) must not purge on
/// every turn. Nothing wakes the loop for it: the age of the last purge is looked at when the
/// counts are, on a turn that runs anyway.
pub const PURGE_EVERY: Duration = Duration::from_secs(2);
/// Destroyed instances that make a purge worth its pause.
pub const PURGE_DEATHS: usize = 200;
/// JS objects allocated beyond the count after the last purge that make one worth its pause.
pub const PURGE_GROWTH: isize = 50_000;

/// Whether the blocking purge runs now: never while something animates, only once `deaths`
/// instances were destroyed or `grown` JS objects added since the last one, and not within
/// [`PURGE_EVERY`] of it (`since`; `None`: there was none yet).
pub fn purge_due(animating: bool, deaths: usize, grown: isize, since: Option<Duration>) -> bool {
    !animating
        && (deaths >= PURGE_DEATHS || grown > PURGE_GROWTH)
        && since.is_none_or(|d| d >= PURGE_EVERY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stale_hidden_window_waits_until_it_is_left_alone() {
        assert!(!stale_relayout_due(1000.0, 1000.0));
        assert!(!stale_relayout_due(
            1000.0,
            1000.0 + STALE_RELAYOUT_MS - 1.0
        ));
        assert!(stale_relayout_due(1000.0, 1000.0 + STALE_RELAYOUT_MS));
        // A change during the wait restarts it.
        assert!(!stale_relayout_due(1200.0, 1000.0 + STALE_RELAYOUT_MS));
    }

    #[test]
    fn purges_need_work_and_are_spaced() {
        let long = Some(PURGE_EVERY);
        assert!(purge_due(false, PURGE_DEATHS, 0, None), "the first one");
        assert!(purge_due(false, PURGE_DEATHS, 0, long));
        assert!(purge_due(false, 0, PURGE_GROWTH + 1, long));
        assert!(
            !purge_due(false, PURGE_DEATHS - 1, PURGE_GROWTH, long),
            "no work"
        );
        assert!(
            !purge_due(true, PURGE_DEATHS, 0, long),
            "never while animating"
        );
        let recent = Some(PURGE_EVERY - Duration::from_millis(1));
        assert!(!purge_due(false, 10 * PURGE_DEATHS, 0, recent), "too soon");
    }

    #[test]
    fn deadlines_are_ordered_by_due_time() {
        let mut d = Deadlines::default();
        assert_eq!(d.timeout(0.0), None, "nothing due: block until an event");
        d.push(f64::INFINITY, Due::Timers);
        assert_eq!(d.next(), None);
        d.push(500.0, Due::Timers);
        d.push(120.0, Due::Caret);
        d.push(900.0, Due::Reap);
        assert_eq!(d.next(), Some((120.0, Due::Caret)));
        assert_eq!(d.timeout(100.0), Some(Duration::from_millis(20)));
        assert_eq!(
            d.timeout(130.0),
            Some(Duration::ZERO),
            "overdue: dispatch without waiting"
        );
        let order: Vec<Due> = d.iter().map(|(_, w)| w).collect();
        assert_eq!(order, [Due::Caret, Due::Timers, Due::Reap]);
    }

    #[test]
    fn idle_has_no_frame_deadline() {
        assert_eq!(
            frame_deadline(10.0, false, false, None, None, Some(0.0), 16.0),
            None
        );
        assert_eq!(
            frame_deadline(10.0, false, true, Some(90.0), Some(8.0), Some(0.0), 16.0),
            None
        );
    }

    #[test]
    fn animating_window_waits_for_its_frame_callback() {
        // The callback wakes the loop: the vsync timer, armed or long overdue, does not. The
        // one deadline is when the wait for the callback is given up.
        let give_up = Some(16.0 + FRAME_CALLBACK_TIMEOUT_MS);
        assert_eq!(
            frame_deadline(20.0, true, true, give_up, Some(16.6), Some(16.0), 16.6),
            Some((116.0, Due::Frame))
        );
        assert_eq!(
            frame_deadline(20.0, true, true, give_up, None, None, 16.6),
            Some((116.0, Due::Frame))
        );
        // Once a window can draw the step again, the vsync timer drives it.
        assert_eq!(
            frame_deadline(20.0, true, false, None, Some(16.6), Some(16.0), 16.6),
            Some((33.2, Due::Frame))
        );
    }

    #[test]
    fn a_withheld_frame_callback_is_given_up_after_its_timeout() {
        // Every animating window was given up on (`awaiting` is false): the step is due now,
        // and the next ones on the vsync timer.
        assert_eq!(
            frame_deadline(116.0, true, false, None, None, Some(16.0), 16.6),
            Some((116.0, Due::Frame))
        );
        assert_eq!(
            frame_deadline(116.5, true, false, None, Some(116.0), Some(116.2), 16.6),
            Some((132.6, Due::Frame))
        );
        // Only endless animations of windows not shown are left: nothing is due, however long
        // the callback is withheld.
        assert_eq!(
            frame_deadline(60_000.0, true, true, None, Some(16.6), Some(16.0), 16.6),
            None
        );
    }

    #[test]
    fn hidden_animation_steps_on_a_vsync_timer() {
        let v = 50.0 / 3.0;
        assert_eq!(
            frame_deadline(20.0, true, false, None, None, Some(16.0), 16.5),
            Some((32.5, Due::Frame)),
            "a new timer fires a vsync after the last frame"
        );
        assert_eq!(
            frame_deadline(50.0, true, false, None, None, Some(16.0), 16.5),
            Some((50.0, Due::Frame)),
            "a late first step runs at once"
        );
        assert_eq!(
            frame_deadline(20.0, true, false, None, None, None, 16.5),
            Some((36.5, Due::Frame))
        );
        // Woken late (or by other work) the timer stays on its grid: 30 steps take 500 ms.
        let mut at = 0.0;
        for _ in 0..30 {
            let now = at + 0.4;
            at = frame_deadline(now, true, false, None, Some(at), Some(now), v)
                .unwrap()
                .0;
        }
        assert!((at - 500.0).abs() < 1e-6, "{at}");
        assert_eq!(
            frame_deadline(5.0, true, false, None, Some(10.0), Some(0.0), v),
            Some((10.0, Due::Frame)),
            "an early wakeup keeps the armed deadline"
        );
    }

    /// Ticks missed while a turn ran: QSGThreadedRenderLoop is two frames ahead of the screen.
    #[test]
    fn ticks_missed_during_a_turn_are_due_at_once_three_at_most() {
        let v = 50.0 / 3.0;
        // The turn of the tick at 10 took 30 ms: the tick of 26.67 is due, the next is on
        // the grid.
        let late = frame_deadline(40.0, true, false, None, Some(10.0), Some(10.0), v).unwrap();
        assert!(
            (late.0 - (10.0 + v)).abs() < 1e-9 && late.0 <= 40.0,
            "{late:?}"
        );
        let next = frame_deadline(40.1, true, false, None, Some(late.0), Some(40.0), v).unwrap();
        assert!(
            (next.0 - (10.0 + 2.0 * v)).abs() < 1e-9 && next.0 > 40.1,
            "{next:?}"
        );
        // It took 100 ms, six ticks: three steps at once, the fourth a vsync after them.
        let (mut at, mut now, mut at_once) = (10.0, 110.0, 0);
        loop {
            at = frame_deadline(now, true, false, None, Some(at), Some(now), v)
                .unwrap()
                .0;
            if at > now {
                break;
            }
            at_once += 1;
            now += 0.1;
        }
        assert_eq!(at_once, 3);
        assert!((at - (110.0 + v)).abs() < 1e-9, "{at}");
    }
}
