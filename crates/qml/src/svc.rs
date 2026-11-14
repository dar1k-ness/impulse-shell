//! Per-service wakeups owned by the `World`: a helper thread that queues work for the main loop
//! sends on its service's channel, which marks only that service ready and pings the loop once.
//! `Runtime::poll_native` then drains the ready services instead of every service on each wake.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SendError};
use std::sync::{Arc, Mutex, OnceLock};

/// A job's stop request: an `AtomicBool` to check between steps, plus wakeups for code that
/// blocks on a channel or a child instead of polling the flag. A stop is QProcess::terminate
/// (the job's programs may get SIGTERM and finish); [`Stop::kill`] is QProcess::kill, which
/// also SIGKILLs every program the job still runs ([`Stop::target`]).
#[derive(Default)]
pub struct Stop {
    flag: AtomicBool,
    killed: AtomicBool,
    wakers: Mutex<Wakers>,
    targets: Mutex<Vec<std::sync::Weak<Target>>>,
}

#[derive(Default)]
struct Wakers {
    next: u64,
    hooks: Vec<(u64, Box<dyn FnOnce() + Send>)>,
}

impl Stop {
    /// Setting the flag runs every registered wakeup once.
    pub fn store(&self, v: bool, o: Ordering) {
        self.flag.store(v, o);
        if v {
            let w = std::mem::take(&mut self.lock().hooks);
            for (_, f) in w {
                f();
            }
            for t in self.live_targets() {
                t.signal(false);
            }
        }
    }

    /// Stops the job and SIGKILLs the programs it runs, as QProcess::kill and ~QProcess.
    pub fn kill(&self) {
        self.killed.store(true, Ordering::Release);
        for t in self.live_targets() {
            t.signal(true);
        }
        self.store(true, Ordering::Release);
    }

    /// Whether [`Stop::kill`] was called.
    pub fn killed(&self) -> bool {
        self.killed.load(Ordering::Acquire)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Wakers> {
        self.wakers.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn hook(&self, f: Box<dyn FnOnce() + Send>) -> Option<u64> {
        let mut w = self.lock();
        if self.flag.load(Ordering::Acquire) {
            drop(w);
            f();
            return None;
        }
        let id = w.next;
        w.next += 1;
        w.hooks.push((id, f));
        Some(id)
    }

    fn unhook(&self, id: Option<u64>) {
        if let Some(id) = id {
            self.lock().hooks.retain(|(i, _)| *i != id);
        }
    }

    /// Run `f` when the stop is requested, at once if it already was, as long as the returned
    /// guard lives: dropping it unregisters `f`, so a long-lived stop does not collect the
    /// wakeups of every wait that ended before it.
    pub fn on_stop(&self, f: impl FnOnce() + Send + 'static) -> OnStop<'_> {
        OnStop {
            id: self.hook(Box::new(f)),
            stop: StopRef::Borrowed(self),
        }
    }

    /// [`Stop::on_stop`] with a guard that owns a reference to the stop, for a registration kept
    /// in a struct or handed to another thread.
    pub fn on_stop_owned(self: &Arc<Self>, f: impl FnOnce() + Send + 'static) -> OnStop<'static> {
        OnStop {
            id: self.hook(Box::new(f)),
            stop: StopRef::Owned(self.clone()),
        }
    }

    /// Blocks for `d` or until the stop is requested, whichever comes first; true when stopped.
    pub fn wait_timeout(&self, d: std::time::Duration) -> bool {
        let woken = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let _hook = {
            let woken = woken.clone();
            self.on_stop(move || {
                *woken.0.lock().unwrap_or_else(|e| e.into_inner()) = true;
                woken.1.notify_all();
            })
        };
        let g = woken.0.lock().unwrap_or_else(|e| e.into_inner());
        let (g, _) = woken
            .1
            .wait_timeout_while(g, d, |stopped| !*stopped)
            .unwrap_or_else(|e| e.into_inner());
        *g
    }

    #[cfg(test)]
    pub(crate) fn wakeups(&self) -> usize {
        self.lock().hooks.len()
    }

    /// Signals `pid` (a `-pgid` for a whole group) from this stop until the returned target is
    /// disarmed or dropped: `on_stop` (if any) on a stop, SIGKILL on a kill; at once if either
    /// already happened. Disarm it before the process is reaped, so a reused id is never hit.
    pub fn target(&self, pid: i32, on_stop: Option<i32>) -> std::sync::Arc<Target> {
        let t = std::sync::Arc::new(Target {
            pid,
            on_stop,
            state: Mutex::new(Armed::Live),
        });
        {
            let mut ts = self.targets.lock().unwrap_or_else(|e| e.into_inner());
            ts.retain(|w| w.strong_count() > 0);
            ts.push(std::sync::Arc::downgrade(&t));
        }
        if self.killed() {
            t.signal(true);
        } else if self.flag.load(Ordering::Acquire) {
            t.signal(false);
        }
        t
    }

    fn live_targets(&self) -> Vec<std::sync::Arc<Target>> {
        let ts = self.targets.lock().unwrap_or_else(|e| e.into_inner());
        ts.iter().filter_map(std::sync::Weak::upgrade).collect()
    }
}

#[derive(PartialEq)]
enum Armed {
    Live,
    Stopped,
    Done,
}

/// A process or group a [`Stop`] signals, see [`Stop::target`].
pub struct Target {
    pid: i32,
    on_stop: Option<i32>,
    state: Mutex<Armed>,
}

impl Target {
    /// The stop's signal (`kill`: SIGKILL), each level at most once and never once disarmed.
    pub fn signal(&self, kill: bool) {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let sig = match (&*st, kill) {
            (Armed::Done, _) => return,
            (_, true) => Some(libc::SIGKILL),
            (Armed::Live, false) => self.on_stop,
            (Armed::Stopped, false) => return,
        };
        if let Some(sig) = sig {
            // SAFETY: kill(2) takes plain integers and touches no memory of this process; the
            // id is still the target's while it is armed (disarmed before the reap).
            unsafe { libc::kill(self.pid, sig) };
        }
        *st = if kill { Armed::Done } else { Armed::Stopped };
    }

    /// No signal from here on; waits for one being sent to finish.
    pub fn disarm(&self) {
        *self.state.lock().unwrap_or_else(|e| e.into_inner()) = Armed::Done;
    }
}

impl Drop for Target {
    fn drop(&mut self) {
        self.disarm();
    }
}

enum StopRef<'a> {
    Borrowed(&'a Stop),
    Owned(Arc<Stop>),
}

/// A wakeup registered with [`Stop::on_stop`]; dropping it unregisters the wakeup.
#[must_use = "dropping the guard unregisters the wakeup at once"]
pub struct OnStop<'a> {
    stop: StopRef<'a>,
    id: Option<u64>,
}

impl OnStop<'_> {
    /// Leaves the wakeup registered until the stop runs it: for one registration per stop that
    /// must outlive the scope that made it (a subscription only the stop ends).
    pub fn keep(mut self) {
        self.id = None;
    }
}

impl Drop for OnStop<'_> {
    fn drop(&mut self) {
        match &self.stop {
            StopRef::Borrowed(s) => s.unhook(self.id),
            StopRef::Owned(s) => s.unhook(self.id),
        }
    }
}

impl std::ops::Deref for Stop {
    type Target = AtomicBool;
    fn deref(&self) -> &AtomicBool {
        &self.flag
    }
}

/// A source of main-loop work. The discriminant is the service's bit in the ready mask.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Svc {
    Hypr,
    /// `hyprctl` calls on their request threads (see `crate::hyprctl`).
    Hyprctl,
    Mpris,
    Sni,
    Pipewire,
    Notif,
    Upower,
    Brightness,
    Bluez,
    Network,
    Pam,
    Polkit,
    Process,
    Keyring,
    Fprint,
    Cava,
    /// GeoClue clients of active `PositionSource`s (see `crate::position`).
    Position,
    /// Retries of failed weather fetches (see `crate::weather`).
    Weather,
    Xhr,
    /// Work the embedding shell queues for itself (image decodes, screencopies, IPC).
    Host,
    /// DesktopEntries scans (see `crate::desktop`); inotify events reach the loop through the
    /// World's epoll set instead (see `crate::fswatch`).
    Fswatch,
    /// The Wayland selection watcher behind `Rustshell.clipboardText`.
    Clipboard,
    /// Finished `SyntaxHighlighter` jobs (see `crate::syntax`).
    Syntax,
    /// Platform theme settings changes: kdeglobals, portal settings (see `crate::colorscheme`).
    Theme,
    /// Replies from threads to `Impulse` types (see `crate::impulse::replies`).
    Impulse,
}

impl Svc {
    pub const fn bit(self) -> u64 {
        1 << self as u8
    }
}

impl crate::runtime::World {
    /// `svc` mirrors properties of `item` to its backend (`PowerProfiles.profile`, a node's
    /// volume): a write to `item` runs it at the next poll, as a setter would reach Quickshell's
    /// service object.
    pub fn mirror(&mut self, item: usize, svc: Svc) {
        *self.svc_watch.entry(item).or_default() |= svc.bit();
    }
}

/// The set of services woken since it was last taken.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ready(pub u64);

impl Ready {
    pub const ALL: Ready = Ready(u64::MAX);

    pub fn has(self, s: Svc) -> bool {
        self.0 & s.bit() != 0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}

#[derive(Default)]
struct Inner {
    ready: AtomicU64,
    ping: OnceLock<Box<dyn Fn() + Send + Sync>>,
}

/// One per `World`: helper threads hold clones through [`Handle`]s and [`Sender`]s.
#[derive(Clone, Default)]
pub struct Waker(Arc<Inner>);

impl Waker {
    /// Installs the embedding loop's wakeup (a calloop ping). Only the first call counts.
    pub fn set_ping(&self, f: impl Fn() + Send + Sync + 'static) {
        let _ = self.0.ping.set(Box::new(f));
    }

    pub fn handle(&self, svc: Svc) -> Handle {
        Handle {
            waker: self.clone(),
            svc,
        }
    }

    /// Marks `svc` ready; pings the loop only if it was not ready yet (one ping per drain).
    #[track_caller]
    pub fn wake(&self, svc: Svc) {
        crate::native::note_wake(std::panic::Location::caller());
        if self.0.ready.fetch_or(svc.bit(), Ordering::AcqRel) & svc.bit() == 0
            && let Some(f) = self.0.ping.get()
        {
            f();
        }
    }

    /// Marks `svc` ready without a ping, for a host that is awake already (its loop saw the
    /// service's fd readable).
    pub fn mark(&self, svc: Svc) {
        self.0.ready.fetch_or(svc.bit(), Ordering::AcqRel);
    }

    pub fn peek(&self) -> Ready {
        Ready(self.0.ready.load(Ordering::Acquire))
    }

    /// Takes the ready set; wakes after this ping again.
    pub fn take(&self) -> Ready {
        Ready(self.0.ready.swap(0, Ordering::AcqRel))
    }

    /// How many clones of this waker are alive (helper threads hold one until they end).
    #[cfg(test)]
    pub(crate) fn clones(&self) -> usize {
        Arc::strong_count(&self.0)
    }

    /// Takes only `svc`'s readiness, leaving the others for the next drain.
    pub fn take_one(&self, svc: Svc) -> bool {
        self.0.ready.fetch_and(!svc.bit(), Ordering::AcqRel) & svc.bit() != 0
    }
}

/// A service's wakeup, passed explicitly into its helper threads.
#[derive(Clone)]
pub struct Handle {
    waker: Waker,
    svc: Svc,
}

impl Handle {
    #[track_caller]
    pub fn wake(&self) {
        self.waker.wake(self.svc)
    }

    pub fn channel<T>(&self) -> (Sender<T>, Receiver<T>) {
        let (tx, rx) = std::sync::mpsc::channel();
        (
            Sender {
                tx,
                handle: self.clone(),
            },
            rx,
        )
    }
}

/// `mpsc::Sender` whose sends wake the owning service.
pub struct Sender<T> {
    tx: std::sync::mpsc::Sender<T>,
    handle: Handle,
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Sender {
            tx: self.tx.clone(),
            handle: self.handle.clone(),
        }
    }
}

impl<T> Sender<T> {
    #[track_caller]
    pub fn send(&self, v: T) -> Result<(), SendError<T>> {
        let r = self.tx.send(v);
        if r.is_ok() {
            self.handle.wake();
        }
        r
    }

    pub fn handle(&self) -> &Handle {
        &self.handle
    }
}

/// A QML method call for one service: the method name as QML spells it and its arguments
/// (the receiving item first for per-object methods).
pub struct Call {
    pub method: String,
    pub args: Vec<crate::value::Value>,
}

impl Call {
    pub fn new(method: impl Into<String>, args: Vec<crate::value::Value>) -> Call {
        Call {
            method: method.into(),
            args,
        }
    }
}

/// A side effect QML requested (`execDetached`, `Hyprland.dispatch`, service methods, ...),
/// routed to its service by variant.
pub enum Effect {
    ExecDetached(Vec<crate::value::Value>),
    HyprDispatch(Vec<crate::value::Value>),
    /// `Rustshell.reload(hard)`: the shell restarts once this frame's effects are done.
    Reload(Vec<crate::value::Value>),
    Session(Call),
    Mpris(Call),
    Sni(Call),
    Notif(Call),
    Bluez(Call),
    Network(Call),
    /// `XMLHttpRequest.send(body)` on `[xhr, body]`.
    Xhr(Vec<crate::value::Value>),
    /// `XMLHttpRequest.abort()` on `[xhr]`: its request in flight is stopped.
    XhrAbort(Vec<crate::value::Value>),
    /// `[item, name, args...]`: an item method no native implements (the host runs window ones).
    Method(Vec<crate::value::Value>),
    /// A `Rustshell` singleton method without a backend (`call`).
    Unhandled(&'static str, Vec<crate::value::Value>),
}

impl Effect {
    /// The effect as the reference trace (`aotcheck`) names it.
    pub fn name(&self) -> String {
        let svc = |p: &str, c: &Call| format!("{p}.{}", c.method);
        match self {
            Effect::ExecDetached(_) => "execDetached".into(),
            Effect::HyprDispatch(_) => "hyprland.dispatch".into(),
            Effect::Reload(_) => "reload".into(),
            Effect::Session(c) => svc("session", c),
            Effect::Mpris(c) => svc("mpris", c),
            Effect::Sni(c) => svc("sni", c),
            Effect::Notif(c) => svc("notif", c),
            Effect::Bluez(c) => svc("bluez", c),
            Effect::Network(c) => svc("network", c),
            Effect::Xhr(_) => "xhr".into(),
            Effect::XhrAbort(_) => "xhr-abort".into(),
            Effect::Method(_) => "method".into(),
            Effect::Unhandled(k, _) => k.to_string(),
        }
    }

    pub fn args(&self) -> &[crate::value::Value] {
        match self {
            Effect::ExecDetached(a)
            | Effect::HyprDispatch(a)
            | Effect::Reload(a)
            | Effect::Xhr(a)
            | Effect::XhrAbort(a)
            | Effect::Method(a)
            | Effect::Unhandled(_, a) => a,
            Effect::Session(c)
            | Effect::Mpris(c)
            | Effect::Sni(c)
            | Effect::Notif(c)
            | Effect::Bluez(c)
            | Effect::Network(c) => &c.args,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    /// A wakeup lives as long as its guard: waits that ended leave nothing behind on a stop
    /// that outlives them (the World's `jobs_stop`), and a live guard still runs at the stop.
    #[test]
    fn dropped_on_stop_guards_leave_no_wakeups() {
        let stop = Arc::new(Stop::default());
        let ran = Arc::new(AtomicUsize::new(0));
        for _ in 0..100 {
            let r = ran.clone();
            drop(stop.on_stop_owned(move || {
                r.fetch_add(1, Ordering::SeqCst);
            }));
        }
        assert_eq!(stop.wakeups(), 0);
        let r = ran.clone();
        let live = stop.on_stop(move || {
            r.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(stop.wakeups(), 1);
        stop.store(true, Ordering::Release);
        assert_eq!(ran.load(Ordering::SeqCst), 1);
        drop(live);
        // Already stopped: runs at once and keeps nothing.
        let r = ran.clone();
        let _late = stop.on_stop(move || {
            r.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!((ran.load(Ordering::SeqCst), stop.wakeups()), (2, 0));
    }

    #[test]
    fn sends_mark_only_their_service_and_ping_once_per_drain() {
        let w = Waker::default();
        let pings = Arc::new(AtomicUsize::new(0));
        let p = pings.clone();
        w.set_ping(move || {
            p.fetch_add(1, Ordering::SeqCst);
        });
        let (tx, rx) = w.handle(Svc::Mpris).channel();
        tx.send(1).unwrap();
        tx.send(2).unwrap();
        assert_eq!(pings.load(Ordering::SeqCst), 1);
        let r = w.take();
        assert!(r.has(Svc::Mpris) && !r.has(Svc::Hypr));
        assert_eq!(rx.try_iter().collect::<Vec<_>>(), vec![1, 2]);
        assert!(w.take().is_empty());
        w.handle(Svc::Hypr).wake();
        assert_eq!(pings.load(Ordering::SeqCst), 2);
        assert!(w.take_one(Svc::Hypr) && w.peek().is_empty());
    }

    #[test]
    fn wakers_are_independent() {
        let (a, b) = (Waker::default(), Waker::default());
        a.handle(Svc::Cava).wake();
        assert!(b.take().is_empty() && a.take().has(Svc::Cava));
    }

    #[test]
    fn failed_send_does_not_wake() {
        let w = Waker::default();
        let (tx, rx) = w.handle(Svc::Sni).channel::<u8>();
        drop(rx);
        assert!(tx.send(1).is_err() && w.take().is_empty());
    }
}

#[cfg(test)]
mod runtime_tests {
    use super::*;
    use crate::runtime::Runtime;

    #[test]
    fn effects_keep_reference_names() {
        let mut rt = Runtime::new(None);
        rt.load_str(
            "import QtQuick\nimport Quickshell\nimport Rustshell.Hyprland\nItem {\n Component.onCompleted: Hyprland.dispatch(\"workspace 2\")\n}\n",
        )
        .unwrap();
        rt.settle();
        let w = rt.world.borrow();
        let got: Vec<_> = w
            .effects
            .iter()
            .map(|e| (e.name(), e.args().len()))
            .collect();
        assert_eq!(got, [("hyprland.dispatch".into(), 1)]);
    }

    #[test]
    fn runtimes_do_not_share_wakeups() {
        let (a, b) = (Runtime::new(None), Runtime::new(None));
        a.world.borrow().waker.handle(Svc::Mpris).wake();
        assert!(a.world.borrow().waker.peek().has(Svc::Mpris));
        assert!(b.world.borrow().waker.peek().is_empty());
    }
}

#[cfg(test)]
mod stop_tests {
    use super::Stop;
    use std::sync::atomic::Ordering;
    use std::sync::mpsc::channel;

    #[test]
    fn stop_wakes_each_waiter_once_and_late_ones_at_once() {
        let s = Stop::default();
        let (tx, rx) = channel();
        let _waiters: Vec<_> = (0..2)
            .map(|i| {
                let tx = tx.clone();
                s.on_stop(move || tx.send(i).unwrap())
            })
            .collect();
        assert!(rx.try_recv().is_err());
        s.store(false, Ordering::Release);
        assert!(rx.try_recv().is_err());
        s.store(true, Ordering::Release);
        assert!(s.load(Ordering::Acquire));
        let mut got: Vec<i32> = rx.try_iter().collect();
        got.sort();
        assert_eq!(got, [0, 1]);
        s.store(true, Ordering::Release);
        assert!(rx.try_recv().is_err());
        let _late = s.on_stop(move || tx.send(2).unwrap());
        assert_eq!(rx.try_recv(), Ok(2));
    }
}
