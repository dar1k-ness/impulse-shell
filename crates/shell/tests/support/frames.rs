//! A test's frames on the runtime's own wakeups: a frame is a tick, the test's layout and a
//! loop turn for every event that is ready, and between two frames the test sleeps as the shell
//! does, until an event (the waker's ping, the epoll set) or the runtime's next deadline. So a
//! test waits for what it waits for (Config's file read, a job's exit), never for a stretch of
//! wall time.
// Shared by several test binaries, each using part of it.
#![allow(dead_code)]

use qml::runtime::Runtime;

pub struct Frames {
    wake: testenv::Wakeups,
    /// The time in ms the runtime's timers see.
    pub now: f64,
}

impl Frames {
    /// Installs the ping. Before anything loads: a service that had work before there was a
    /// ping does not ping for more.
    pub fn new(rt: &mut Runtime) -> Frames {
        let wake = testenv::Wakeups::new();
        rt.world.borrow().waker.set_ping(wake.ping());
        Frames { wake, now: 1000.0 }
    }

    fn turn(&mut self, rt: &mut Runtime, woke: testenv::Woke) {
        if woke.fd {
            rt.process_fd_ready();
        }
        rt.poll_native();
        rt.settle();
    }

    pub fn frame(&mut self, rt: &mut Runtime, layout: &mut impl FnMut(&mut Runtime)) {
        self.now += 16.0;
        rt.tick(self.now);
        rt.settle();
        layout(rt);
        rt.settle();
        loop {
            let woke = self
                .wake
                .wait(rt.process_fd(), Some(std::time::Duration::ZERO));
            if !woke.any() {
                return;
            }
            self.turn(rt, woke);
        }
    }

    pub fn frames(&mut self, rt: &mut Runtime, layout: &mut impl FnMut(&mut Runtime), n: usize) {
        for _ in 0..n {
            self.frame(rt, layout);
        }
    }

    /// Frames until `done`.
    pub fn until(
        &mut self,
        rt: &mut Runtime,
        layout: &mut impl FnMut(&mut Runtime),
        mut done: impl FnMut(&mut Runtime) -> bool,
    ) {
        loop {
            self.frame(rt, layout);
            if done(rt) {
                return;
            }
            let next = rt.next_deadline(self.now);
            let slept = std::time::Instant::now();
            let woke = self.wake.wait(
                rt.process_fd(),
                next.is_finite()
                    .then(|| std::time::Duration::from_secs_f64(next / 1000.0)),
            );
            self.now += slept.elapsed().as_secs_f64() * 1000.0;
            if woke.any() {
                self.turn(rt, woke);
            }
        }
    }

    /// Frames until the Config singleton has read its file.
    pub fn until_config(&mut self, rt: &mut Runtime, layout: &mut impl FnMut(&mut Runtime)) {
        self.until(rt, layout, |rt| {
            let mut w = rt.world.borrow_mut();
            let config = *w.singletons.get("Config").expect("the Config singleton");
            w.read(config, "ready").is_some_and(|v| v.truthy())
        });
    }
}
