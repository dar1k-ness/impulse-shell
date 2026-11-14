//! A test's runtime loop on a virtual clock. It runs a loop turn for every event the runtime
//! is woken by (its waker's ping, its epoll set: process I/O and inotify) and for every timer,
//! jumping the clock to the next timer's deadline instead of sleeping to it, so a test waits
//! for what it waits for, never for a stretch of wall time.
// Shared by several test binaries, each using part of it.
#![allow(dead_code)]

use qml::runtime::Runtime;
use std::time::Duration;
use testenv::Wakeups;

pub struct Loop {
    wake: Wakeups,
    ep: Option<i32>,
    /// The virtual time in ms, what the runtime's timers see.
    pub now: f64,
}

impl Loop {
    /// Becomes `rt`'s host loop: installs its waker ping and watches its epoll set.
    pub fn new(rt: &mut Runtime) -> Loop {
        let wake = Wakeups::new();
        rt.world.borrow().waker.set_ping(wake.ping());
        let ep = rt.process_fd();
        Loop { wake, ep, now: 0.0 }
    }

    /// Runs loop turns until nothing is ready at `now`: no pool work in flight, no wakeup,
    /// no event on the epoll set and no timer due.
    pub fn drain(&mut self, rt: &mut Runtime) {
        loop {
            workpool::wait_idle();
            rt.poll_native();
            rt.tick(self.now);
            rt.poll_native();
            rt.settle();
            // Pool work this turn started has finished and pinged before the wakeups are read.
            workpool::wait_idle();
            let woke = self.wake.wait(self.ep, Some(Duration::ZERO));
            if woke.fd {
                rt.process_fd_ready();
            }
            if !woke.any() && rt.next_deadline(self.now) > 0.0 {
                return;
            }
        }
    }

    /// Runs the loop until `done`; false once nothing is left that could change it (no event
    /// ready and no timer pending).
    pub fn until(&mut self, rt: &mut Runtime, mut done: impl FnMut(&mut Runtime) -> bool) -> bool {
        loop {
            self.drain(rt);
            if done(rt) {
                return true;
            }
            let d = rt.next_deadline(self.now);
            if !d.is_finite() {
                return false;
            }
            self.now += d;
        }
    }

    /// Runs the loop until `done` without moving the clock, blocking for the next event (a
    /// ping, the epoll set) whenever nothing is ready: for what a child process or another
    /// thread brings in its own time.
    pub fn wait_until(&mut self, rt: &mut Runtime, mut done: impl FnMut(&mut Runtime) -> bool) {
        loop {
            self.drain(rt);
            if done(rt) {
                return;
            }
            if self.wake.wait(self.ep, None).fd {
                rt.process_fd_ready();
            }
        }
    }

    /// Runs the loop through the next `ms` of virtual time, each timer at its deadline.
    pub fn advance(&mut self, rt: &mut Runtime, ms: f64) {
        let end = self.now + ms;
        loop {
            self.drain(rt);
            let d = rt.next_deadline(self.now);
            if self.now + d > end {
                self.now = end;
                self.drain(rt);
                return;
            }
            self.now += d;
        }
    }

    /// One loop turn at `now`, repeated while it changed something: what a poll writes counts
    /// only for the next one. Waits for no pool job, so a transfer in flight cannot hold it.
    fn turn(&mut self, rt: &mut Runtime) {
        loop {
            let rev = rt.revision();
            rt.poll_native();
            rt.tick(self.now);
            rt.poll_native();
            rt.settle();
            if rt.revision() == rev {
                return;
            }
        }
    }

    /// Runs loop turns, sleeping on the runtime's wakeups and epoll set (no timeout) between
    /// them, until `done`. The clock stays at `now`: for flows driven by I/O (a transfer, a
    /// process), where the event that ends the wait wakes the runtime.
    pub fn block_until(&mut self, rt: &mut Runtime, mut done: impl FnMut(&mut Runtime) -> bool) {
        loop {
            self.turn(rt);
            if done(rt) {
                return;
            }
            if self.wake.wait(self.ep, None).fd {
                rt.process_fd_ready();
            }
        }
    }

    /// Runs loop turns while a wakeup or epoll event is already ready, without waiting for
    /// pool work: delivers what has arrived (a process's last output after its exit).
    pub fn flush(&mut self, rt: &mut Runtime) {
        loop {
            self.turn(rt);
            let woke = self.wake.wait(self.ep, Some(Duration::ZERO));
            if woke.fd {
                rt.process_fd_ready();
            }
            if !woke.any() {
                return;
            }
        }
    }
}
