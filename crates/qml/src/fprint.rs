//! The `Fingerprint` singleton of the `Impulse` module (`qmltypes/Impulse/fingerprint.qmltypes`),
//! from fprintd over the system bus.
//!
//! `enrolled` is what `fprintd-list USER` reports: a reader is there and lists a finger of the
//! user. It is checked on a worker thread when the singleton is created. `waitForAvailable()`
//! follows the events that can make fingerprint unlock possible again ([`watch`]): fprintd
//! taking its bus name and a USB device being added. Each updates `enrolled`, and the first
//! that leaves it true ends the wait with `available()`. LockContext.qml waits like that
//! after a fingerprint conversation that could not start a verify.

use crate::runtime::World;
use crate::svc::Stop;
use crate::value::Value;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;
use zbus::blocking::{Connection, Proxy};
use zbus::proxy::{CacheProperties, MethodFlags};
use zbus::zvariant::OwnedObjectPath;

const DEST: &str = "net.reactivated.Fprint";
const NO_PRINTS: &str = "net.reactivated.Fprint.Error.NoEnrolledPrints";

/// `fprintd-list USER` printed "Fingerprints for user" and exited 0: there is a device, every
/// device answered `ListEnrolledFingers` (NoEnrolledPrints: none) and one listed a finger.
pub fn fingerprints_configured(user: &str) -> bool {
    crate::dbus::system().is_some_and(|c| configured_on(&c, user, true))
}

/// [`fingerprints_configured`] over `conn`. Without `activate` the calls carry the D-Bus
/// `NO_AUTO_START` flag: an fprintd that is not running is not started, and counts as none.
fn configured_on(conn: &Connection, user: &str, activate: bool) -> bool {
    let flags = if activate {
        Default::default()
    } else {
        MethodFlags::NoAutoStart.into()
    };
    let proxy = |path: &str, iface: &'static str| -> zbus::Result<Proxy<'_>> {
        zbus::blocking::proxy::Builder::new(conn)
            .destination(DEST)?
            .path(path.to_string())?
            .interface(iface)?
            .cache_properties(CacheProperties::No)
            .build()
    };
    let Ok(Some(devices)) = proxy(
        "/net/reactivated/Fprint/Manager",
        "net.reactivated.Fprint.Manager",
    )
    .and_then(|m| m.call_with_flags::<_, _, Vec<OwnedObjectPath>>("GetDevices", flags, &())) else {
        return false;
    };
    let mut any = false;
    for d in &devices {
        match proxy(d.as_str(), "net.reactivated.Fprint.Device").and_then(|p| {
            p.call_with_flags::<_, _, Vec<String>>("ListEnrolledFingers", flags, &(user,))
        }) {
            Ok(f) => any |= f.is_some_and(|f| !f.is_empty()),
            Err(zbus::Error::MethodError(name, _, _)) if name.as_str() == NO_PRINTS => {}
            Err(_) => return false,
        }
    }
    any
}

/// `whoami`: the effective user's name.
pub fn user_name() -> String {
    // SAFETY: geteuid(2) takes no arguments and cannot fail.
    crate::polkit::passwd_name(unsafe { libc::geteuid() })
        .map(|(n, _)| n)
        .unwrap_or_default()
}

/// A [`watch`]'s report: (watch id, a finger is enrolled, after an event).
type Woke = (u64, bool, bool);

pub struct Fprint {
    tx: crate::svc::Sender<bool>,
    rx: Receiver<bool>,
    wtx: crate::svc::Sender<Woke>,
    wrx: Receiver<Woke>,
    /// The first check is still running.
    checking: bool,
    /// The wait `waitForAvailable()` started: its id and its watch.
    wait: Option<(u64, Watch)>,
    next: u64,
}

impl Fprint {
    /// The first check finished.
    pub fn idle(&self) -> bool {
        !self.checking
    }

    /// `waitForAvailable()` is waiting for fprintd or a reader.
    pub fn waiting(&self) -> bool {
        self.wait.is_some()
    }
}

fn state(w: &mut World) -> &mut Fprint {
    w.fprint.get_or_insert_with(|| {
        let h = w.waker.handle(crate::svc::Svc::Fprint);
        let (tx, rx) = h.channel();
        let (wtx, wrx) = h.channel();
        Fprint {
            tx,
            rx,
            wtx,
            wrx,
            checking: false,
            wait: None,
            next: 0,
        }
    })
}

/// The `Fingerprint` singleton was created: checks `enrolled` once.
fn setup(w: &mut World, _id: usize) {
    let f = state(w);
    f.checking = true;
    let (tx, refused) = (f.tx.clone(), f.tx.clone());
    if crate::native::sandboxed() {
        // fprintd is not asked: no finger is enrolled.
        let _ = tx.send(false);
        return;
    }
    let r = std::thread::Builder::new()
        .name("ri-fprint".into())
        .spawn(move || {
            let _ = tx.send(fingerprints_configured(&user_name()));
        });
    if let Err(e) = r {
        elog::eprintln!("Fingerprint: cannot start the thread for the first check: {e}");
        // The check ends unanswered, as one that found no enrolled finger.
        let _ = refused.send(false);
    }
}

fn with_world(it: &mut crate::eval::Interp, f: impl FnOnce(&mut World)) {
    if let Some(w) = crate::runtime::world_of(it) {
        f(&mut w.borrow_mut());
    }
}

/// The methods of the `Fingerprint` singleton.
const METHODS: &[(&str, crate::eval::NativeFn)] = &[
    ("waitForAvailable", |it, _, _| {
        // Under a spawn hook nothing watches the bus: no reader ever becomes available.
        if crate::impulse::hooked("Fingerprint", "waitForAvailable", &[]) {
            return Ok(Value::Undefined);
        }
        with_world(it, |w| {
            let f = state(w);
            f.next += 1;
            let (id, tx) = (f.next, f.wtx.clone());
            // The earlier wait's watch, if any, ends here; its reports are stale by id.
            f.wait = Some((id, watch(id, tx)));
        });
        Ok(Value::Undefined)
    }),
    ("cancelWait", |it, _, _| {
        with_world(it, |w| {
            if let Some(f) = w.fprint.as_mut() {
                f.wait = None;
            }
        });
        Ok(Value::Undefined)
    }),
];

pub static TYPE: crate::impulse::Type = crate::impulse::Type {
    name: "Fingerprint",
    methods: METHODS,
    init: Some(setup),
};

/// The first check's answer and the wait's reports, as `enrolled` and `available()`.
pub(crate) fn poll(w: &mut World) -> bool {
    let Some(f) = w.fprint.as_mut() else {
        return false;
    };
    let done: Vec<_> = f.rx.try_iter().collect();
    let woke: Vec<_> = f.wrx.try_iter().collect();
    if !done.is_empty() {
        f.checking = false;
    }
    let Some(&me) = w.singletons.get("Fingerprint") else {
        return false;
    };
    for &ok in &done {
        w.write(me, "enrolled", Value::Bool(ok));
    }
    for &(id, ok, event) in &woke {
        let f = w.fprint.as_mut().unwrap();
        if f.wait.as_ref().is_none_or(|(i, _)| *i != id) {
            continue;
        }
        if event && ok {
            f.wait = None;
        }
        w.write(me, "enrolled", Value::Bool(ok));
        if event && ok {
            crate::native::emit(w, me, "available", &[], vec![]);
        }
    }
    !done.is_empty() || !woke.is_empty()
}

/// Watches as wait `id` until dropped. It reports what fprintd says now,
/// then again after each event that can change it: fprintd taking `net.reactivated.Fprint`
/// (started, restarted or activated by any client; NameOwnerChanged with a new owner) and a
/// USB device being added (a reader plugged in, which fprintd picks up when asked). Checks after
/// a bus event never start fprintd (`NO_AUTO_START`), so a check of ours is never itself the
/// next appearance; one after a USB add does, as a new reader is only seen through fprintd.
pub struct Watch {
    stop: Arc<Stop>,
    /// The uevent thread's stop pipe: dropping it ends the thread.
    _uevents: Option<OwnedFd>,
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn watch(id: u64, tx: crate::svc::Sender<Woke>) -> Watch {
    let stop = Arc::new(Stop::default());
    if let Some(owner) = crate::sysbus::owner_rule(DEST) {
        let (tx, st) = (tx.clone(), stop.clone());
        crate::sysbus::spawn(vec![owner], stop.clone(), move |conn, owners| {
            let report = |event| {
                !st.load(Ordering::Acquire)
                    && tx
                        .send((id, configured_on(conn, &user_name(), false), event))
                        .is_ok()
            };
            // The subscription is live: an fprintd appearing from here on is reported below.
            if !report(false) {
                return false;
            }
            for m in owners.iter() {
                if crate::sysbus::owner_changed(&m).is_some_and(|(_, owned)| owned) && !report(true)
                {
                    return false;
                }
            }
            true
        });
    }
    let st = stop.clone();
    let uevents = watch_usb(move || {
        !st.load(Ordering::Acquire)
            && tx
                .send((id, fingerprints_configured(&user_name()), true))
                .is_ok()
    });
    Watch {
        stop,
        _uevents: uevents,
    }
}

/// udev `add` uevents of USB devices; `added` runs after each batch and returns false to stop.
/// Returns the stop pipe's write end: dropping it ends the thread.
fn watch_usb(added: impl Fn() -> bool + Send + 'static) -> Option<OwnedFd> {
    let mut p = [0; 2];
    // SAFETY: `p` is the writable array of two descriptors pipe2(2) fills.
    if unsafe { libc::pipe2(p.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return None;
    }
    // SAFETY: pipe2 just returned this read end, owned by nothing else.
    let stop = unsafe { OwnedFd::from_raw_fd(p[0]) };
    // SAFETY: pipe2 just returned this write end, owned by nothing else.
    let stop_w = unsafe { OwnedFd::from_raw_fd(p[1]) };
    let r = std::thread::Builder::new()
        .name("ri-fprint-usb".into())
        .spawn(move || {
            let listen = || {
                let ctx = udev::Context::new()?;
                let mut b = udev::MonitorBuilder::new(&ctx)?;
                b.match_subsystem_devtype("usb", "usb_device")?;
                b.listen()
            };
            let mut socket = match listen() {
                Ok(s) => s,
                Err(e) => {
                    elog::eprintln!("fingerprint reader uevents: {e}");
                    return;
                }
            };
            let mut fds = [socket.as_raw_fd(), stop.as_raw_fd()].map(|fd| libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            });
            loop {
                // SAFETY: `fds` is a writable array of `fds.len()` pollfds over open descriptors.
                if unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) } < 0 {
                    if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                        continue;
                    }
                    return;
                }
                if fds[1].revents != 0 || fds[0].revents & !libc::POLLIN != 0 {
                    return;
                }
                // The socket is non-blocking: drain every queued event, check once.
                let adds = socket
                    .by_ref()
                    .filter(|e| matches!(e.event_type(), udev::EventType::Add))
                    .count();
                if adds > 0 && !added() {
                    return;
                }
            }
        });
    if let Err(e) = r {
        elog::eprintln!("Fingerprint: cannot start the thread for reader uevents: {e}");
        return None;
    }
    Some(stop_w)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first check, its thread refused, ends as one that found no enrolled finger, with
    /// the refusal in the log. In a process of its own: the test reads what that process wrote
    /// to its stderr.
    #[test]
    fn a_refused_thread_ends_the_first_check() {
        const TEST: &str = concat!(module_path!(), "::a_refused_thread_ends_the_first_check");
        if !crate::testenv::in_own_process(TEST) {
            crate::testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
            return;
        }
        let mut rt = crate::runtime::Runtime::new(None);
        rt.load_str("import QtQuick\nItem {}").unwrap();
        rt.settle();
        assert!(rt.world.borrow().fprint.is_none());
        crate::testenv::refuse_new_threads();
        let mut root = 0;
        let text = crate::testenv::stderr_of(|| {
            // The singleton is created here, and its first check starts.
            root = rt
                .load_str(
                    "import QtQuick\nimport Impulse\n\
                     Item { property var enrolled: Fingerprint.enrolled }",
                )
                .unwrap();
            rt.settle();
            rt.poll_native();
            rt.settle();
        });
        let checking = rt.world.borrow().fprint.as_ref().map(|f| !f.idle());
        assert_eq!(checking, Some(false), "the first check never ended");
        assert_eq!(rt.get(root, "enrolled").to_js_string(), "false");
        assert_eq!(
            text.matches("Fingerprint: cannot start the thread for the first check:")
                .count(),
            1,
            "stderr: {text:?}"
        );
        assert_eq!(rt.errors(), Vec::<String>::new());
    }

    /// The thread for reader uevents refused: no watch on them, and the refusal is in the log.
    #[test]
    fn a_refused_uevent_thread_is_logged() {
        const TEST: &str = concat!(module_path!(), "::a_refused_uevent_thread_is_logged");
        if !crate::testenv::in_own_process(TEST) {
            crate::testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
            return;
        }
        crate::testenv::refuse_new_threads();
        let mut watched = true;
        let text = crate::testenv::stderr_of(|| watched = watch_usb(|| true).is_some());
        assert!(!watched);
        assert_eq!(
            text.matches("Fingerprint: cannot start the thread for reader uevents:")
                .count(),
            1,
            "stderr: {text:?}"
        );
    }
}
