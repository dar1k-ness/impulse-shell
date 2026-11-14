//! A test's host loop for a `Runtime` on a virtual clock (ee_equiv, probe_qml,
//! ports_ab). It runs a loop turn for every event the runtime is woken by (its waker's ping,
//! its epoll set: process I/O and inotify) and sleeps until the next one while a Process or a
//! request is in flight; only with nothing in flight does the clock jump to the next timer, so
//! a test waits for what it waits for, never for a stretch of wall time.
// Shared by several test binaries, each using part of it.
#![allow(dead_code)]

use qml::runtime::Runtime;
use std::time::Duration;
use testenv::Wakeups;

pub struct Host {
    wake: Wakeups,
    ep: Option<i32>,
    /// The virtual time in ms, what the runtime's timers see.
    pub now: f64,
}

impl Host {
    /// Becomes `rt`'s host loop: installs its waker ping and watches its epoll set.
    pub fn new(rt: &mut Runtime) -> Host {
        let wake = Wakeups::new();
        rt.world.borrow().waker.set_ping(wake.ping());
        let ep = rt.process_fd();
        Host { wake, ep, now: 0.0 }
    }

    fn turn(&self, rt: &mut Runtime) {
        rt.poll_native();
        rt.tick(self.now);
        rt.poll_native();
        rt.settle();
    }

    /// A Process runs or a request waits for its answer: their end is an event to come.
    fn in_flight(rt: &Runtime) -> bool {
        let w = rt.world.borrow();
        !w.procs.is_empty() || !w.xhr.is_empty()
    }

    /// Waits for the next event (`None`: only looks); true when one came.
    fn event(&self, rt: &Runtime, block: bool) -> bool {
        let woke = self.wake.wait(self.ep, (!block).then_some(Duration::ZERO));
        if woke.fd {
            rt.process_fd_ready();
        }
        woke.any()
    }

    /// Runs turns until `done`. While nothing is in flight the clock jumps to the next timer;
    /// with no timer either, it sleeps until an event (pool work ending pings the waker).
    pub fn until(&mut self, rt: &mut Runtime, mut done: impl FnMut(&mut Runtime) -> bool) {
        loop {
            self.turn(rt);
            if done(rt) {
                return;
            }
            if self.event(rt, false) {
                continue;
            }
            let d = rt.next_deadline(self.now);
            if Self::in_flight(rt) || !d.is_finite() {
                self.event(rt, true);
            } else {
                self.now += d;
            }
        }
    }

    /// Runs `ms` of virtual time, each timer at its deadline and every event as it comes, and
    /// returns once nothing is in flight or ready at its end.
    pub fn advance(&mut self, rt: &mut Runtime, ms: f64) {
        let end = self.now + ms;
        loop {
            self.turn(rt);
            if self.event(rt, false) {
                continue;
            }
            if Self::in_flight(rt) {
                self.event(rt, true);
                continue;
            }
            let d = rt.next_deadline(self.now);
            if self.now + d > end {
                if self.now == end {
                    return;
                }
                self.now = end;
            } else {
                self.now += d;
            }
        }
    }

    /// [`Host::advance`] by nothing: every Process and request started so far has ended and
    /// its handlers ran.
    pub fn settle(&mut self, rt: &mut Runtime) {
        self.advance(rt, 0.0);
    }

    /// [`Host::settle`], and the pool's work too (detached commands until they exit, jobs and
    /// what they hand on, `workpool::wait_idle`): returns once none of it is left and no
    /// event came meanwhile.
    pub fn idle(&mut self, rt: &mut Runtime) {
        loop {
            self.settle(rt);
            workpool::wait_idle();
            if !self.event(rt, false) && !Self::in_flight(rt) {
                return;
            }
        }
    }
}
