//! The main loop of the headless tools, on a virtual clock: time moves only to the runtime's
//! next timer, animation frame or service deadline, so a scenario runs the same at any host
//! load. External work (process output, watched files, D-Bus) is taken when it arrives; a
//! scenario that needs it blocks on the runtime's fds until it has.
use crate::runtime::Runtime;
use std::os::fd::{AsRawFd, OwnedFd};
use std::process::{Child, Command};

/// The virtual epoch `Date.now()` starts at (2026-01-01T12:00:00Z), as in aotcheck.
const T0: f64 = 1_767_268_800_000.0;
/// One frame at 60 Hz: views apply model changes at their window's polish, once per frame.
const FRAME: f64 = 1000.0 / 60.0;

/// Work a tool runs after every turn.
type AfterTurn = Box<dyn FnMut(&mut Runtime)>;

pub struct Driver {
    /// Virtual ms since the start.
    now: f64,
    /// Frame the last polish ran in.
    polished: Option<u64>,
    /// Polish the windows that exist at each turn (`None`: polish off or set once by the tool).
    windows: bool,
    /// The tool's own work at the end of every turn (sceneflood's scene layout).
    after_turn: Option<AfterTurn>,
}

impl Driver {
    /// `windows`: register the current Panel/Floating/Popup windows for polish at every turn.
    pub fn new(windows: bool) -> Self {
        crate::builtins::set_clock(Some(T0));
        Self {
            now: 0.0,
            polished: None,
            windows,
            after_turn: None,
        }
    }

    /// Runs `f` at the end of every turn, after the polish.
    pub fn after_turn(mut self, f: impl FnMut(&mut Runtime) + 'static) -> Self {
        self.after_turn = Some(Box::new(f));
        self
    }

    fn frame(&self) -> u64 {
        (self.now / FRAME) as u64
    }

    /// One main-loop turn at the current virtual time: native services, timers and animations,
    /// then the held windows' polish once per frame. Whether animations or held windows want the
    /// next frame.
    pub fn turn(&mut self, rt: &mut Runtime) -> bool {
        if self.windows {
            let wins: Vec<usize> = {
                let mut w = rt.world.borrow_mut();
                ["PanelWindow", "FloatingWindow", "PopupWindow"]
                    .iter()
                    .flat_map(|t| w.of_type(t))
                    .collect()
            };
            rt.set_polish_wins(wins);
        }
        rt.poll_native();
        let busy = rt.tick(self.now);
        let held = rt.held_windows();
        if !held.is_empty() && self.polished != Some(self.frame()) {
            self.polished = Some(self.frame());
            rt.polish(&held);
        }
        if let Some(f) = &mut self.after_turn {
            f(rt);
        }
        busy || !held.is_empty()
    }

    /// When the runtime needs its next turn (virtual ms): now while services have work, else
    /// the next frame while `busy`, else its next timer or service deadline.
    fn next(&mut self, rt: &mut Runtime, busy: bool) -> f64 {
        if rt.native_ready() {
            return self.now;
        }
        let mut at = self.now + rt.next_deadline(self.now);
        if busy {
            // `now` may sit a rounding error below the frame boundary it was advanced to.
            let mut frame = (self.frame() + 1) as f64 * FRAME;
            if frame <= self.now {
                frame += FRAME;
            }
            at = at.min(frame);
        }
        at
    }

    fn advance(&mut self, to: f64) {
        self.now = to;
        crate::builtins::set_clock(Some(T0 + to));
    }

    /// Runs `ms` of virtual time.
    pub fn run(&mut self, rt: &mut Runtime, ms: f64) {
        self.run_while(rt, ms, |_| true);
    }

    /// Runs up to `ms` of virtual time, stopping at the first turn where the runtime is idle
    /// (no animation, no held window, nothing due now). Whether it went idle.
    pub fn settle(&mut self, rt: &mut Runtime, ms: f64) -> bool {
        let mut idle = false;
        self.run_while(rt, ms, |busy| {
            idle = !busy;
            busy
        });
        idle
    }

    fn run_while(&mut self, rt: &mut Runtime, ms: f64, mut more: impl FnMut(bool) -> bool) {
        let end = self.now + ms;
        loop {
            let busy = self.turn(rt);
            let at = self.next(rt, busy);
            if at > self.now && !more(busy) {
                return;
            }
            if at > end {
                self.advance(end);
                return;
            }
            self.advance(at);
        }
    }

    /// Turns until `done` holds. Virtual time does not move: what `done` waits for comes from
    /// outside (a process, a file, the bus), so between turns this blocks on the runtime's fds
    /// and on `extra` (the tool's own children's pidfds, ...).
    pub fn until(
        &mut self,
        rt: &mut Runtime,
        extra: &[i32],
        mut done: impl FnMut(&mut Runtime) -> bool,
    ) {
        loop {
            self.turn(rt);
            if done(rt) {
                return;
            }
            if !rt.native_ready() && rt.next_deadline(self.now) > 0.0 {
                rt.wait_native_or(extra);
            }
        }
    }
}

/// Children a tool runs alongside the runtime (D-Bus clients whose calls the runtime answers,
/// ...): the tool turns the runtime while they run, and their exits are events on pidfds that
/// [`Driver::until`] blocks on together with the runtime's fds.
#[derive(Default)]
pub struct Children {
    live: Vec<(Child, OwnedFd)>,
    /// Children that exited successfully.
    pub ok: usize,
}

impl Children {
    pub fn spawn(&mut self, mut c: Command) {
        let child = c
            .spawn()
            .unwrap_or_else(|e| panic!("{:?}: {e}", c.get_program()));
        let pid = i32::try_from(child.id()).expect("pid");
        let fd = workpool::pidfd_open(pid).expect("pidfd_open");
        self.live.push((child, fd));
    }

    /// How many still run.
    pub fn len(&self) -> usize {
        self.live.len()
    }

    pub fn is_empty(&self) -> bool {
        self.live.is_empty()
    }

    fn fds(&self) -> Vec<i32> {
        self.live.iter().map(|(_, fd)| fd.as_raw_fd()).collect()
    }

    /// Collects the children whose pidfds report their exit; whether any had exited.
    fn reap(&mut self) -> bool {
        let mut fds: Vec<libc::pollfd> = self
            .fds()
            .into_iter()
            .map(|fd| libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            })
            .collect();
        // SAFETY: `fds` is a live array of `fds.len()` pollfds for the duration of the call.
        unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, 0) };
        let before = self.live.len();
        for (i, p) in fds.iter().enumerate().rev() {
            if p.revents != 0 {
                let (mut c, _) = self.live.remove(i);
                if c.wait().is_ok_and(|s| s.success()) {
                    self.ok += 1;
                }
            }
        }
        self.live.len() < before
    }

    /// Turns the runtime until at most `max` children still run.
    pub fn drain(&mut self, drv: &mut Driver, rt: &mut Runtime, max: usize) {
        while self.live.len() > max {
            let fds = self.fds();
            drv.until(rt, &fds, |_| self.reap());
        }
    }
}
