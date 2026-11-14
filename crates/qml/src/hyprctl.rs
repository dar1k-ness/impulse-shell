//! Requests to Hyprland and hyprsunset for the QML types `HyprlandRequest` and
//! `HyprsunsetClient` (module `Impulse`, `qmltypes/Impulse/hyprland_request.qmltypes` and
//! `hyprsunset_client.qmltypes`). A request goes straight to the Hyprland / hyprsunset socket
//! off the main thread (Hyprland requests one after another on one lane, hyprsunset requests
//! on another, in order); its reply comes back on the main loop as the type's signal.

use crate::runtime::{Runtime, World};
use crate::value::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

// ---- requests ----

/// One step of a call, run in order on the call's thread; its stdout is the step's output.
pub enum Job {
    /// `hyprctl` request on `.socket.sock` (wire form, e.g. `j/clients`, `/dispatch ...`).
    Hypr(String),
    /// `hyprctl hyprsunset ...` request on `.hyprsunset.sock`, sent once the hyprsunset starts
    /// asked for before it settled (`crate::probe::sunset`).
    Sunset(String, u64),
}

/// What a step got: the reply, or why there is none.
type Reply = Result<String, String>;

type Done = Box<dyn FnOnce(&mut World, Vec<Reply>)>;

/// A world's `hyprctl` calls whose replies are still out (in `World::hyprctl`).
pub struct Calls {
    next: u64,
    pending: HashMap<u64, (Option<usize>, Option<Done>)>,
    busy: HashSet<usize>,
    /// Latest-wins calls by `Process` item, started once that item is idle (`start_latest`).
    queued: Vec<(usize, Vec<Job>, Option<Done>)>,
    tx: crate::svc::Sender<(u64, Vec<Reply>)>,
    rx: Receiver<(u64, Vec<Reply>)>,
}

const NO_INSTANCE: &str = "no Hyprland instance (HYPRLAND_INSTANCE_SIGNATURE is not set)";

/// `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE`, as hyprctl resolves it.
pub(crate) fn instance_dir() -> Option<PathBuf> {
    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
    let rt = std::env::var("XDG_RUNTIME_DIR")
        // SAFETY: getuid(2) takes no arguments and cannot fail.
        .unwrap_or_else(|_| format!("/run/user/{}", unsafe { libc::getuid() }));
    Some(Path::new(&rt).join("hypr").join(sig))
}

/// hyprctl's `SO_RCVTIMEO` on the `.socket.sock` connection (`request` in hyprctl/src/main.cpp).
const RCVTIMEO: std::time::Duration = std::time::Duration::from_secs(5);

/// The reply to request `cmd` on socket `sock` of instance `dir`, or the step that failed.
fn ctl(dir: Option<&Path>, sock: &str, cmd: &str) -> Reply {
    use crate::hypr::IpcError;
    let p = dir.ok_or(NO_INSTANCE)?.join(sock);
    crate::hypr::exchange(&p, cmd, Some(RCVTIMEO)).map_err(|e| match e {
        IpcError::Connect(e) => format!("cannot connect to {}: {e}", p.display()),
        IpcError::Write(e) => format!("cannot send the request: {e}"),
        IpcError::Read(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
            format!("no reply from Hyprland in {} s", RCVTIMEO.as_secs())
        }
        IpcError::Read(e) => format!("cannot read the reply: {e}"),
    })
}

fn exec(dir: Option<&Path>, j: &Job) -> Reply {
    match j {
        Job::Hypr(c) => ctl(dir, ".socket.sock", c),
        Job::Sunset(c, ticket) => {
            crate::probe::sunset::wait(*ticket);
            let d = dir.ok_or(NO_INSTANCE)?;
            Ok(crate::hypr::request_ipc(&d.join(".hyprsunset.sock"), c))
        }
    }
}

fn calls(w: &mut World) -> &mut Calls {
    let h = w.waker.handle(crate::svc::Svc::Hyprctl);
    w.hyprctl.get_or_insert_with(|| {
        let (tx, rx) = h.channel();
        Calls {
            next: 0,
            pending: HashMap::new(),
            busy: HashSet::new(),
            queued: Vec::new(),
            tx,
            rx,
        }
    })
}

/// `start` for an item whose reply replaces state: the call starts on the next poll, or
/// once the `busy` item's running call has finished, and a later call before that replaces
/// it. A burst of requests costs one call, and the last request is never lost.
pub fn start_latest(w: &mut World, busy: usize, jobs: Vec<Job>, done: Option<Done>) {
    let c = calls(w);
    match c.queued.iter_mut().find(|q| q.0 == busy) {
        Some(q) => *q = (busy, jobs, done),
        None => c.queued.push((busy, jobs, done)),
    }
    w.waker.wake(crate::svc::Svc::Hyprctl);
}

/// Run `jobs` off the main thread, then `done` with their outputs on the main loop. With
/// `busy` (the replaced `Process` item) a call while the previous one runs is dropped, as
/// `running = true` on a running Process is. Returns whether the call started.
pub fn start(w: &mut World, busy: Option<usize>, jobs: Vec<Job>, done: Option<Done>) -> bool {
    let dir = instance_dir();
    let c = calls(w);
    if let Some(b) = busy
        && !c.busy.insert(b)
    {
        return false;
    }
    c.next += 1;
    let id = c.next;
    c.pending.insert(id, (busy, done));
    let tx = c.tx.clone();
    let place = place(&jobs);
    place(Box::new(move || {
        let outs = jobs.iter().map(|j| exec(dir.as_deref(), j)).collect();
        let _ = tx.send((id, outs));
    }));
    true
}

/// Where a call runs. One with a hyprsunset request runs on the hyprsunset lane: it may wait
/// for hyprsunset to start, and hyprsunset applies requests in the order its connections come
/// in (a thread per call let a `temperature` read made after a `temperature N` reach it first
/// and answer the stale temperature). One with a Hyprland request runs on the Hyprland lane
/// ([`crate::hypr::submit`]): its reply has no deadline, so it must not hold a bounded worker
/// that logind and D-Bus actions queue for.
fn place(jobs: &[Job]) -> fn(Box<dyn FnOnce() + Send>) {
    static SUNSET: workpool::Lane = workpool::Lane::new();
    if jobs.iter().any(|j| matches!(j, Job::Sunset(..))) {
        |f| SUNSET.run(f)
    } else {
        |f| crate::hypr::submit(f)
    }
}

/// Calls whose replies are still out (tests).
pub fn in_flight(w: &World) -> usize {
    w.hyprctl
        .as_ref()
        .map_or(0, |c| c.pending.len() + c.queued.len())
}

impl Runtime {
    /// Apply finished `hyprctl` calls; returns whether any finished.
    pub(crate) fn poll_hyprctl(&mut self) -> bool {
        let done: Vec<_> = match self.world.borrow().hyprctl.as_ref() {
            Some(c) => c.rx.try_iter().collect(),
            None => return false,
        };
        let mut any = !done.is_empty();
        for (id, outs) in done {
            let p = self.world.borrow_mut().hyprctl.as_mut().and_then(|c| {
                let p = c.pending.remove(&id);
                if let Some((Some(b), _)) = &p {
                    c.busy.remove(b);
                }
                p
            });
            if let Some((_, Some(f))) = p {
                f(&mut self.world.borrow_mut(), outs);
            }
        }
        let idle: Vec<_> = match self.world.borrow_mut().hyprctl.as_mut() {
            Some(c) => {
                let (idle, wait) = std::mem::take(&mut c.queued)
                    .into_iter()
                    .partition(|q| !c.busy.contains(&q.0));
                c.queued = wait;
                idle
            }
            None => Vec::new(),
        };
        for (b, jobs, done) in idle {
            any |= start(&mut self.world.borrow_mut(), Some(b), jobs, done);
        }
        any
    }
}

// ---- the QML types ----

fn arg_text(args: &[Value]) -> String {
    args.first().map(|v| v.to_js_string()).unwrap_or_default()
}

/// `temperature N` requests made so far: a `temperature` read made before the latest one
/// answers a temperature already replaced.
static SUNSET_SETS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn sunset(cmd: String) -> Job {
    Job::Sunset(cmd, crate::probe::sunset::ticket())
}

/// `HyprlandRequest.send()`: `command` as hyprctl sends it (`j/` with `json`). It goes out at
/// once; while the item's previous reply is out it is kept instead, the latest one winning.
/// `replied(text)` ends every request: Hyprland's reply with `error` empty, or an empty text
/// with `error` saying why there is no reply.
fn request_send(w: &mut World, item: usize) {
    let command = match w.read(item, "command") {
        Some(v) => v.to_js_string(),
        None => String::new(),
    };
    let json = w.read(item, "json").is_some_and(|v| v.truthy());
    let wire = format!("{}/{command}", if json { "j" } else { "" });
    let done: Done = Box::new(move |w, mut out| {
        let (text, error) = match out.swap_remove(0) {
            Ok(text) => (text, String::new()),
            Err(error) => (String::new(), error),
        };
        w.write(item, "error", Value::str(&error));
        crate::native::emit(w, item, "replied", &["text"], vec![Value::str(&text)]);
    });
    let jobs = vec![Job::Hypr(wire)];
    if calls(w).busy.contains(&item) {
        start_latest(w, item, jobs, Some(done));
    } else {
        start(w, Some(item), jobs, Some(done));
    }
}

/// `HyprsunsetClient.start()`: start hyprsunset unless one runs; requests made after it wait
/// for that start to settle (`crate::probe::sunset`).
fn sunset_start(_w: &mut World) {
    let t = crate::probe::sunset::ask();
    // A test's spawn hook stands in for the start: requests waiting on it must not hang.
    if crate::native::hooked_call(SUNSET_CLIENT.name, "start", &[]).is_some() {
        crate::probe::sunset::settle(t);
        return;
    }
    workpool::run(move || {
        crate::probe::sunset::ensure(Path::new("hyprsunset"), t);
    });
}

/// `HyprsunsetClient.readTemperature()`: one read at a time; its answer is dropped when a
/// `setTemperature` was made after it was asked for, also one made between the answer's
/// arrival and the delivery of `temperatureRead` ([`sunset_set`]).
fn sunset_read(w: &mut World, item: usize) {
    let sets = SUNSET_SETS.load(std::sync::atomic::Ordering::Relaxed);
    let done: Done = Box::new(move |w, out| {
        if SUNSET_SETS.load(std::sync::atomic::Ordering::Relaxed) != sets {
            return;
        }
        // No answer reads as an empty one.
        let text = out[0].as_deref().unwrap_or_default();
        let text = text.strip_suffix('\n').unwrap_or(text);
        crate::native::emit(
            w,
            item,
            "temperatureRead",
            &["text"],
            vec![Value::str(text)],
        );
    });
    start(
        w,
        Some(item),
        vec![sunset("temperature".into())],
        Some(done),
    );
}

/// `HyprsunsetClient.setTemperature()`. A `temperatureRead` that is queued but not delivered
/// yet answers the temperature this one replaces: handlers of property changes run before
/// queued signals, so a handler of the same loop turn can set the temperature in between
/// (Hyprsunset.qml turning night light on as the config becomes ready), and the answer
/// delivered after it switched the toggle off under a night light that is on.
fn sunset_set(w: &mut World, item: usize, temperature: String) {
    crate::native::save_pending_config(w);
    SUNSET_SETS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    w.emits
        .retain(|(i, sig, _)| !(*i == item && sig == "temperatureRead"));
    let job = sunset(format!("temperature {temperature}"));
    start(w, None, vec![job], None);
}

/// `HyprlandRequest` of the `Impulse` module.
pub static REQUEST: crate::impulse::Element = crate::impulse::Element {
    methods: &[("send", |w, item, _| {
        request_send(w, item);
        Value::Undefined
    })],
    ..crate::impulse::Element::new("HyprlandRequest")
};

/// `HyprsunsetClient` of the `Impulse` module.
pub static SUNSET_CLIENT: crate::impulse::Element = crate::impulse::Element {
    methods: &[
        ("readTemperature", |w, item, _| {
            sunset_read(w, item);
            Value::Undefined
        }),
        ("setGamma", |w, _, args| {
            crate::native::save_pending_config(w);
            let job = sunset(format!("gamma {}", arg_text(args)));
            start(w, None, vec![job], None);
            Value::Undefined
        }),
        ("setTemperature", |w, item, args| {
            sunset_set(w, item, arg_text(args));
            Value::Undefined
        }),
        ("start", |w, _, _| {
            sunset_start(w);
            Value::Undefined
        }),
    ],
    ..crate::impulse::Element::new("HyprsunsetClient")
};

#[cfg(test)]
mod tests {
    use super::{Job, exec, place};

    /// Hyprland accepts the request connections but never answers (its main loop is stuck in
    /// a modeset or a long `reload`). Calls waiting on it must leave the bounded workers to the
    /// one-off actions that share them (logind poweroff and suspend, D-Bus calls).
    #[test]
    fn a_stuck_hyprland_leaves_the_bounded_workers_free() {
        let dir = std::env::temp_dir().join(format!("hypr-{}-stuck", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let listener = std::os::unix::net::UnixListener::bind(dir.join(".socket.sock")).unwrap();
        let (held_tx, held) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for c in listener.incoming() {
                // Held unanswered while the test runs, closed (EOF, an empty reply) after.
                let _ = held_tx.send(c.unwrap());
            }
        });
        for _ in 0..=workpool::WORKERS {
            let jobs = vec![Job::Hypr("j/devices".into())];
            let d = dir.clone();
            place(&jobs)(Box::new(move || {
                let _ = exec(Some(&d), &jobs[0]);
            }));
        }
        let first = held.recv().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        workpool::run(move || tx.send(()).unwrap());
        rx.recv().unwrap();
        drop((first, held));
        workpool::wait_idle();
        let _ = std::fs::remove_dir_all(&dir);
    }

    use std::io::Read;
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixListener;
    use std::path::Path;
    use std::sync::mpsc::{Receiver, Sender};

    /// A fake hyprsunset and the test's handles on it.
    struct Sunset {
        dir: std::path::PathBuf,
        /// One per request read.
        asked: Receiver<()>,
        /// Lets the main loop finish the request read last: its reply is written then.
        handled: Sender<()>,
        /// Whether each reply write reached the client (`false`: it had closed its end, where
        /// the real hyprsunset, with SIGPIPE's default disposition, is killed).
        replied: Receiver<bool>,
    }

    impl Sunset {
        /// Lets one request through: waits for it, has it handled, and checks its reply.
        fn serve_one(&self) {
            self.asked.recv().unwrap();
            self.handled.send(()).unwrap();
            assert!(
                self.replied.recv().unwrap(),
                "the client closed before the reply"
            );
        }
    }

    /// hyprsunset's IPC thread (IPCSocket.cpp): one connection at a time, kept open for the
    /// next request until the client closes it; each reply is written only once the main
    /// loop handled the request, which here is when the test says so.
    fn fake_hyprsunset(name: &str) -> Sunset {
        let dir = std::env::temp_dir().join(format!("sunset-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let l = UnixListener::bind(dir.join(".hyprsunset.sock")).unwrap();
        let (atx, asked) = std::sync::mpsc::channel();
        let (handled, htx) = std::sync::mpsc::channel();
        let (rtx, replied) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for mut c in l.incoming().flatten() {
                let mut buf = [0u8; 1024];
                while c.read(&mut buf).is_ok_and(|n| n > 0) {
                    let _ = atx.send(());
                    if htx.recv().is_err() {
                        return;
                    }
                    // SAFETY: the fd is the live stream `c`; the 2 bytes come from a static.
                    let n = unsafe { libc::write(c.as_raw_fd(), b"ok".as_ptr().cast(), 2) };
                    let _ = rtx.send(n == 2);
                }
            }
        });
        Sunset {
            dir,
            asked,
            handled,
            replied,
        }
    }

    #[test]
    fn sunset_waits_for_a_busy_hyprsunset() {
        let f = fake_hyprsunset("busy");
        let dir = f.dir.clone();
        let call = std::thread::spawn(move || exec(Some(&dir), &Job::Sunset("gamma 50".into(), 0)));
        f.serve_one();
        assert_eq!(call.join().unwrap().as_deref(), Ok("ok\n"));
    }

    /// Dragging the brightness slider into the gamma range sends `gamma N` per step.
    #[test]
    fn sunset_gamma_flood_never_closes_before_the_reply() {
        let f = fake_hyprsunset("flood");
        let calls: Vec<_> = (0..30)
            .map(|i| {
                let dir = f.dir.clone();
                std::thread::spawn(move || exec(Some(&dir), &Job::Sunset(format!("gamma {i}"), 0)))
            })
            .collect();
        for _ in 0..30 {
            f.serve_one();
        }
        for c in calls {
            assert_eq!(c.join().unwrap().as_deref(), Ok("ok\n"));
        }
    }

    #[test]
    fn hypr_without_hyprland_says_it_cannot_connect() {
        let dir = std::env::temp_dir().join(format!("hypr-{}-none", std::process::id()));
        let out = exec(Some(&dir), &Job::Hypr("j/monitors".into()));
        let sock = dir.join(".socket.sock");
        let reason = out.unwrap_err();
        assert!(
            reason.starts_with(&format!("cannot connect to {}: ", sock.display())),
            "{reason}"
        );
    }

    /// No `HYPRLAND_INSTANCE_SIGNATURE`: neither request has a socket to go to.
    #[test]
    fn no_instance_is_an_error_not_a_reply() {
        for job in [
            Job::Hypr("j/monitors".into()),
            Job::Sunset("temperature".into(), 0),
        ] {
            assert_eq!(exec(None, &job), Err(super::NO_INSTANCE.to_string()));
        }
    }

    /// A wedged Hyprland accepts the request and never answers: hyprctl's 5 s `SO_RCVTIMEO`
    /// ends the call with its timeout message, and the connection is closed.
    #[test]
    fn hypr_unanswered_request_times_out_like_hyprctl() {
        let dir = std::env::temp_dir().join(format!("hypr-{}-wedged", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let l = UnixListener::bind(dir.join(".socket.sock")).unwrap();
        let wedged = std::thread::spawn(move || {
            let (mut c, _) = l.accept().unwrap();
            let mut req = Vec::new();
            let mut buf = [0u8; 1024];
            // Holds the connection until the client closes it.
            while let Ok(n @ 1..) = c.read(&mut buf) {
                req.extend_from_slice(&buf[..n]);
            }
            req
        });
        let out = exec(Some(&dir), &Job::Hypr("j/clients".into()));
        assert_eq!(out, Err("no reply from Hyprland in 5 s".to_string()));
        assert_eq!(wedged.join().unwrap(), b"j/clients");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sunset_without_hyprsunset_is_couldnt_connect() {
        let dir = std::env::temp_dir().join(format!("sunset-{}-none", std::process::id()));
        let out = exec(Some(&dir), &Job::Sunset("temperature".into(), 0));
        let sock = dir.join(".hyprsunset.sock");
        assert_eq!(
            out,
            Ok(format!("Couldn't connect to {}. (3)\n", sock.display()))
        );
    }

    /// A fresh start after `pkill hyprsunset`: the socket file of the killed one is left, two
    /// `startHyprsunset()` and a request come at once. A second hyprsunset would bind its own
    /// socket and exit (Hyprland refuses a second CTM manager), and a request sent before the
    /// started one binds would hit the stale file.
    #[test]
    fn sunset_requests_wait_for_the_one_hyprsunset_started() {
        use crate::probe::sunset;
        let dir = std::env::temp_dir().join(format!("sunset-{}-start", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let sock = dir.join(".hyprsunset.sock");
        drop(UnixListener::bind(&sock).unwrap());
        let name = format!("fakesunset{}", std::process::id());
        let fake = dir.join("fake.py");
        std::fs::write(
            &fake,
            r#"import os, socket, sys
sock, lock, gate = sys.argv[1:4]
assert sys.argv[4:] == ["--verbose"], sys.argv
# Wayland roundtrip and CTM reload come before the IPC thread: they last until the test
# closes the gate (refused once it is gone).
try:
    g = socket.socket(socket.AF_UNIX)
    g.connect(gate)
    g.recv(1)
except OSError:
    pass
try:
    os.close(os.open(lock, os.O_CREAT | os.O_EXCL))
    first = True
except FileExistsError:
    first = False
try:
    os.unlink(sock)
except FileNotFoundError:
    pass
s = socket.socket(socket.AF_UNIX)
s.bind(sock)
s.listen(10)
print(f"[LOG] hyprsunset socket started at {sock} (fd: {s.fileno()})", flush=True)
if not first:
    sys.exit(1)
while True:
    c, _ = s.accept()
    while c.recv(1024):
        c.sendall(b"ok")
    c.close()
"#,
        )
        .unwrap();
        let prog = fake_sunset(
            &dir,
            &name,
            &fake,
            &[&sock, &dir.join("lock"), &dir.join("gate")],
        );
        let gate = UnixListener::bind(dir.join("gate")).unwrap();
        let starts: Vec<_> = [sunset::ask(), sunset::ask()]
            .into_iter()
            .map(|t| {
                let prog = prog.clone();
                std::thread::spawn(move || sunset::ensure(&prog, t))
            })
            .collect();
        let t = sunset::ticket();
        let d = dir.clone();
        let req =
            std::thread::spawn(move || exec(Some(&d), &Job::Sunset("temperature 4000".into(), t)));
        // The started hyprsunset is in its startup: let it (and any other) bind.
        drop(gate.accept().unwrap());
        drop(gate);
        assert_eq!(req.join().unwrap().as_deref(), Ok("ok\n"));
        for s in starts {
            s.join().unwrap();
        }
        let running = crate::osutil::pidof(&name);
        assert_eq!(running.len(), 1, "{running:?}");
        assert_eq!(
            exec(Some(&dir), &Job::Sunset("gamma 80".into(), 0)).as_deref(),
            Ok("ok\n")
        );
        for p in running {
            // SAFETY: kill(2) takes plain integers and touches no memory of this process.
            unsafe { libc::kill(p, libc::SIGTERM) };
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Installs `dir/name`, running the Python `fake` with `args` (then what it is called with)
    /// under the process name `name`.
    fn fake_sunset(dir: &Path, name: &str, fake: &Path, args: &[&Path]) -> std::path::PathBuf {
        // The interpreter itself, not what `python3` on PATH runs: a wrapper there (a pyenv
        // shim) execs the interpreter again under its own name, so `pidof` would never see
        // the fake by `name`.
        let python = std::process::Command::new("python3")
            .args(["-c", "import sys; print(sys.executable)"])
            .output()
            .unwrap();
        assert!(python.status.success());
        let python = String::from_utf8(python.stdout).unwrap();
        let mut cmd = format!(
            "exec -a {name} '{}' '{}'",
            python.trim_end(),
            fake.display()
        );
        for a in args {
            cmd += &format!(" '{}'", a.display());
        }
        let prog = dir.join(name);
        crate::testenv::install_script(&prog, &format!("#!/bin/bash\n{cmd} \"$@\"\n"));
        prog
    }

    /// hyprsunset creates its socket file with bind(2) and listens only after it: a request
    /// sent in between was refused ("Couldn't connect"). A start settles on the line its IPC
    /// thread logs once it listens; its later log lines neither block nor kill it once nobody
    /// reads them (SIGPIPE ignored).
    #[test]
    fn sunset_requests_wait_until_hyprsunset_listens() {
        use crate::probe::sunset;
        let dir = std::env::temp_dir().join(format!("sunset-{}-listen", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let sock = dir.join(".hyprsunset.sock");
        let name = format!("fakesunsetl{}", std::process::id());
        let fake = dir.join("fake.py");
        std::fs::write(
            &fake,
            r#"import socket, sys
sock, gate = sys.argv[1:3]
assert sys.argv[3:] == ["--verbose"], sys.argv
with open("/proc/self/status") as f:
    ign = next(int(l.split()[1], 16) for l in f if l.startswith("SigIgn:"))
assert ign & (1 << 12), "SIGPIPE not ignored"
print("[LOG] connecting", flush=True)
s = socket.socket(socket.AF_UNIX)
s.bind(sock)
# Between bind and listen until the test closes the gate.
g = socket.socket(socket.AF_UNIX)
g.connect(gate)
g.recv(1)
s.listen(10)
print(f"[LOG] hyprsunset socket started at {sock} (fd: {s.fileno()})", flush=True)
while True:
    c, _ = s.accept()
    while c.recv(1024):
        c.sendall(b"ok")
    c.close()
    # Nobody reads stdout any more: far past a pipe's capacity.
    for _ in range(64):
        try:
            print("[LOG] " + "x" * 4096, flush=True)
        except BrokenPipeError:
            pass
"#,
        )
        .unwrap();
        let prog = fake_sunset(&dir, &name, &fake, &[&sock, &dir.join("gate")]);
        let gate = UnixListener::bind(dir.join("gate")).unwrap();
        let t = sunset::ask();
        let start = std::thread::spawn(move || sunset::ensure(&prog, t));
        // Bound, not listening.
        let held = gate.accept().unwrap();
        assert!(sock.exists());
        let d = dir.clone();
        let req = std::thread::spawn(move || {
            exec(
                Some(&d),
                &Job::Sunset("temperature 4000".into(), sunset::ticket()),
            )
        });
        drop(held);
        drop(gate);
        assert_eq!(req.join().unwrap().as_deref(), Ok("ok\n"));
        start.join().unwrap();
        for cmd in ["gamma 80", "temperature 5000"] {
            assert_eq!(
                exec(Some(&dir), &Job::Sunset(cmd.into(), 0)).as_deref(),
                Ok("ok\n")
            );
        }
        let running = crate::osutil::pidof(&name);
        assert_eq!(running.len(), 1, "{running:?}");
        for p in running {
            // SAFETY: kill(2) takes plain integers and touches no memory of this process.
            unsafe { libc::kill(p, libc::SIGTERM) };
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
