//! [`Sys`] on the real session: Hyprland's request socket, dconf and the settings portal over
//! D-Bus, notifications over D-Bus, signals through `/proc`, and child processes only for
//! the external programs (mpvpaper, ffmpeg, kdialog, hyprpicker).

use crate::sys::{OnAction, Sys};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use zbus::zvariant::Value;

const IFACE: &str = "org.gnome.desktop.interface";

pub struct Live;

fn hypr_socket() -> Option<PathBuf> {
    let rt = std::env::var_os("XDG_RUNTIME_DIR")?;
    let sig = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")?;
    Some(Path::new(&rt).join("hypr").join(sig).join(".socket.sock"))
}

/// Every process but this one: `(pid, comm, argv)`.
pub(crate) fn processes() -> Vec<(i32, String, Vec<String>)> {
    let me = std::process::id() as i32;
    let Ok(rd) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    rd.flatten()
        .filter_map(|e| {
            let pid: i32 = e.file_name().to_str()?.parse().ok()?;
            if pid == me {
                return None;
            }
            let comm = std::fs::read_to_string(e.path().join("comm"))
                .ok()?
                .trim_end()
                .to_string();
            let raw = std::fs::read(e.path().join("cmdline")).ok()?;
            let argv = raw
                .split(|b| *b == 0)
                .filter(|a| !a.is_empty())
                .map(|a| String::from_utf8_lossy(a).into_owned())
                .collect();
            Some((pid, comm, argv))
        })
        .collect()
}

/// How long [`Sys::kill_exact_wait`] waits for its targets to exit.
const KILL_DEADLINE: Duration = Duration::from_secs(2);

/// How long [`Sys::write_tty`] waits on a terminal that takes no output (its buffer is full
/// and nobody reads it, or its output is stopped with Ctrl-S) before it leaves the rest.
const TTY_STALL: Duration = Duration::from_millis(500);

/// Writes all of `data` to `fd`, which does not block: when it takes no more (`EAGAIN`), waits
/// until it is writable again and writes on, so the data arrives whole. Gives up once `fd` has
/// taken nothing for `stall`, or on an error: `false`, the rest is not written.
pub fn write_whole(fd: BorrowedFd<'_>, mut data: &[u8], stall: Duration) -> bool {
    while !data.is_empty() {
        // SAFETY: write(2) of the `data.len()` bytes at `data` to an fd that outlives the call.
        let n = unsafe { libc::write(fd.as_raw_fd(), data.as_ptr().cast(), data.len()) };
        if n > 0 {
            data = &data[n as usize..];
            continue;
        }
        if n == 0 {
            return false;
        }
        match std::io::Error::last_os_error().kind() {
            std::io::ErrorKind::Interrupted => continue,
            std::io::ErrorKind::WouldBlock => {}
            _ => return false,
        }
        let Ok(gave_up) = timer_after(stall) else {
            return false;
        };
        let mut polls = [
            libc::pollfd {
                fd: fd.as_raw_fd(),
                events: libc::POLLOUT,
                revents: 0,
            },
            libc::pollfd {
                fd: gave_up.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        loop {
            // SAFETY: `polls` is a valid array of two pollfds on fds that outlive the call.
            let n = unsafe { libc::poll(polls.as_mut_ptr(), 2, -1) };
            if n >= 0 {
                break;
            }
            if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
                return false;
            }
        }
        // Writable, or an error or hangup that the next write reports; else the time is up.
        if polls[0].revents == 0 {
            return false;
        }
    }
    true
}

/// `comm` is the kernel's 15-byte name of the executable, `argv[0]` what the process was started as.
fn exe_is(comm: &str, argv: &[String], name: &str) -> bool {
    comm == name || argv.first().map(|a| a.rsplit('/').next().unwrap_or(a)) == Some(name)
}

/// Whether the process that has `pid` now `is` the one meant, by its `comm` and argv.
fn proc_is(pid: i32, is: &dyn Fn(&str, &[String]) -> bool) -> bool {
    let dir = PathBuf::from(format!("/proc/{pid}"));
    let Ok(comm) = std::fs::read_to_string(dir.join("comm")) else {
        return false;
    };
    let argv: Vec<String> = std::fs::read(dir.join("cmdline"))
        .unwrap_or_default()
        .split(|b| *b == 0)
        .filter(|a| !a.is_empty())
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .collect();
    is(comm.trim_end(), &argv)
}

/// Send `sig` to every process whose executable is exactly `name` and wait until each has
/// exited (its pidfd turns readable) or `deadline` passes: `true` when all exited in time.
/// Signals go through the pidfd, so a pid reused after the `/proc` scan is never hit.
pub fn kill_exact_wait(name: &str, sig: i32, deadline: Duration) -> bool {
    let fds = signal_exact(name, sig);
    if fds.is_empty() {
        return true;
    }
    match timer_after(deadline) {
        Ok(timer) => wait_exits(&fds, timer.as_fd()),
        Err(_) => false,
    }
}

/// [`kill_exact_wait`] with the deadline as an event: gives up once `give_up` turns readable
/// (a timerfd firing, the far end of a pipe closing).
pub fn kill_exact_wait_until(name: &str, sig: i32, give_up: BorrowedFd<'_>) -> bool {
    wait_exits(&signal_exact(name, sig), give_up)
}

/// Sends `sig` to every process whose executable is exactly `name`; their pidfds.
fn signal_exact(name: &str, sig: i32) -> Vec<OwnedFd> {
    signal_scanned(&processes(), sig, &|comm, argv| exe_is(comm, argv, name))
}

/// Sends `sig` to every process of the scan `procs` (as [`processes`] gives it) that `is` the
/// one meant by its `comm` and argv, and still is when the signal goes out; their pidfds.
/// The scan is old by then: a process of it may have exited and its pid gone to another one.
/// So the process is held by a pidfd, looked at again, and signalled through the pidfd.
#[doc(hidden)]
pub fn signal_scanned(
    procs: &[(i32, String, Vec<String>)],
    sig: i32,
    is: &dyn Fn(&str, &[String]) -> bool,
) -> Vec<OwnedFd> {
    let mut fds: Vec<OwnedFd> = Vec::new();
    for (pid, comm, argv) in procs {
        let pid = *pid;
        if !is(comm, argv) {
            continue;
        }
        // SAFETY: pidfd_open(2) takes a pid and flags and returns a new fd or -1.
        let raw = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
        if raw < 0 {
            continue;
        }
        // SAFETY: `raw` is a freshly opened fd owned by nobody else.
        let fd = unsafe { OwnedFd::from_raw_fd(raw as i32) };
        // The pid may have been reused between the scan and pidfd_open.
        if !proc_is(pid, is) {
            continue;
        }
        // SAFETY: pidfd_send_signal(2) on an fd we own, without siginfo.
        let sent = unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                fd.as_raw_fd(),
                sig,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            )
        };
        if sent == 0 {
            fds.push(fd);
        }
    }
    fds
}

/// A monotonic timerfd that turns readable once `after` has passed.
fn timer_after(after: Duration) -> std::io::Result<OwnedFd> {
    // SAFETY: timerfd_create(2) takes a clock id and flags and returns a new fd or -1.
    let raw = unsafe { libc::timerfd_create(libc::CLOCK_MONOTONIC, libc::TFD_CLOEXEC) };
    if raw < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `raw` is a freshly opened fd owned by nobody else.
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    // An all-zero it_value disarms the timer instead of firing it at once.
    let after = after.max(Duration::from_nanos(1));
    let spec = libc::itimerspec {
        it_interval: libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        },
        it_value: libc::timespec {
            tv_sec: libc::time_t::try_from(after.as_secs()).unwrap_or(libc::time_t::MAX),
            tv_nsec: after.subsec_nanos().into(),
        },
    };
    // SAFETY: timerfd_settime(2) on the timerfd we own, with a valid itimerspec and no old one.
    if unsafe { libc::timerfd_settime(fd.as_raw_fd(), 0, &spec, std::ptr::null_mut()) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(fd)
}

/// Blocks until every pidfd in `fds` turns readable (`true`) or `give_up` does first (`false`).
fn wait_exits(fds: &[OwnedFd], give_up: BorrowedFd<'_>) -> bool {
    let pollfd = |fd: i32| libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let mut polls: Vec<libc::pollfd> = fds.iter().map(|fd| pollfd(fd.as_raw_fd())).collect();
    while !polls.is_empty() {
        polls.push(pollfd(give_up.as_raw_fd()));
        // SAFETY: `polls` is a valid array of `polls.len()` pollfds on fds that outlive the call.
        let n = unsafe { libc::poll(polls.as_mut_ptr(), polls.len() as libc::nfds_t, -1) };
        if n < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return false;
        }
        let gave_up = polls.pop().is_some_and(|p| p.revents != 0);
        polls.retain(|p| p.revents == 0);
        if gave_up && !polls.is_empty() {
            return false;
        }
    }
    true
}

fn dconf_key(key: &str) -> String {
    format!("/{}/{key}", IFACE.replace('.', "/"))
}

/// A string key from a dconf database, as `gsettings get` sees it when the user set it.
pub fn dconf_read(db: &Path, key: &str) -> Option<String> {
    let file = gvdb::read::File::from_file(db).ok()?;
    let table = file.hash_table().ok()?;
    let mut v = table.get_value(key).ok()?;
    while let Value::Value(inner) = v {
        v = *inner;
    }
    String::try_from(v).ok()
}

/// A dconf changeset (`a{smv}`) for one key, as `dconf_changeset_serialize` builds it.
pub fn dconf_change(key: &str, value: &str) -> zbus::zvariant::Result<Vec<u8>> {
    use zbus::zvariant::serialized::Context;
    let mut m = std::collections::HashMap::new();
    m.insert(key.to_string(), Some(Value::from(value)));
    // dconf only speaks GVariant; zvariant 5 still serializes it.
    #[allow(deprecated)]
    let ctx = Context::new_gvariant(zbus::zvariant::LE, 0);
    Ok(zbus::zvariant::to_bytes(ctx, &m)?.to_vec())
}

impl Sys for Live {
    fn hypr_json(&self, cmd: &str) -> Option<serde_json::Value> {
        serde_json::from_str(&self.hypr_request(&format!("j/{cmd}"))?).ok()
    }

    fn hypr_request(&self, cmd: &str) -> Option<String> {
        hypripc::exchange(&hypr_socket()?, cmd, None).ok()
    }

    fn gsettings_get(&self, key: &str) -> Option<String> {
        dconf_read(&dirs::config_dir()?.join("dconf/user"), &dconf_key(key))
    }

    fn gsettings_set(&self, key: &str, value: &str) {
        let Ok(blob) = dconf_change(&dconf_key(key), value) else {
            return;
        };
        if let Some(c) = bus::session() {
            let _ = c.call_method(
                Some("ca.desrt.dconf"),
                "/ca/desrt/dconf/Writer/user",
                Some("ca.desrt.dconf.Writer"),
                "Change",
                &(blob,),
            );
        }
    }

    fn run(&self, argv: &[String], cwd: Option<&Path>) -> Option<String> {
        let mut c = workpool::userenv::command(&argv[0]);
        c.args(&argv[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(d) = cwd {
            c.current_dir(d);
        }
        let out = c.output().ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn spawn(&self, argv: &[String]) {
        let mut c = workpool::userenv::command(&argv[0]);
        c.args(&argv[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Ok(ch) = c.spawn() {
            workpool::reap(ch);
        }
    }

    fn has_command(&self, name: &str) -> bool {
        use std::os::unix::fs::PermissionsExt;
        std::env::var_os("PATH").is_some_and(|p| {
            std::env::split_paths(&p).any(|d| {
                std::fs::metadata(d.join(name))
                    .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            })
        })
    }

    fn kill_exact_wait(&self, name: &str, sig: i32) {
        kill_exact_wait(name, sig, KILL_DEADLINE);
    }

    fn signal_named(&self, name: &str, sig: i32) {
        let procs = processes();
        if !procs.iter().any(|(_, _, a)| a.join(" ").contains(name)) {
            return;
        }
        signal_scanned(&procs, sig, &|comm, argv| exe_is(comm, argv, name));
    }

    fn ttys(&self) -> Vec<PathBuf> {
        let Ok(rd) = std::fs::read_dir("/dev/pts") else {
            return Vec::new();
        };
        let mut v: Vec<PathBuf> = rd
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            })
            .map(|e| e.path())
            .collect();
        v.sort();
        v
    }

    fn write_tty(&self, tty: &Path, data: &[u8]) {
        use std::os::unix::fs::OpenOptionsExt;
        // Not blocking, so a terminal that takes no output cannot hold the caller for good.
        if let Ok(f) = std::fs::OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NOCTTY | libc::O_NONBLOCK)
            .open(tty)
        {
            write_whole(f.as_fd(), data, TTY_STALL);
        }
    }

    fn notify(
        &self,
        app: &str,
        category: Option<&str>,
        summary: &str,
        body: &str,
        actions: &[(&str, &str)],
        then: OnAction,
    ) {
        match bus::session() {
            Some(c) => notify_on(c.inner(), app, category, summary, body, actions, then),
            None => then(None),
        }
    }
}

/// Sends the notification and returns: the reply and the action/close signal are awaited on the
/// connection's executor, so no thread sits waiting for the user; `then` runs on a worker.
pub fn notify_on(
    conn: &zbus::Connection,
    app: &str,
    category: Option<&str>,
    summary: &str,
    body: &str,
    actions: &[(&str, &str)],
    then: OnAction,
) {
    let conn2 = conn.clone();
    let (app, summary, body) = (app.to_owned(), summary.to_owned(), body.to_owned());
    let category = category.map(str::to_owned);
    let acts: Vec<String> = actions
        .iter()
        .flat_map(|(k, l)| [k.to_string(), l.to_string()])
        .collect();
    conn.executor()
        .spawn(
            async move {
                let key =
                    notify_async(&conn2, &app, category.as_deref(), &summary, &body, &acts).await;
                workpool::long(move || then(key));
            },
            "notify",
        )
        .detach();
}

async fn notify_async(
    conn: &zbus::Connection,
    app: &str,
    category: Option<&str>,
    summary: &str,
    body: &str,
    acts: &[String],
) -> Option<String> {
    use futures_lite::StreamExt;
    let mut signals = if acts.is_empty() {
        None
    } else {
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface("org.freedesktop.Notifications")
            .ok()?
            .build();
        Some(
            zbus::MessageStream::for_match_rule(rule, conn, None)
                .await
                .ok()?,
        )
    };
    let acts: Vec<&str> = acts.iter().map(String::as_str).collect();
    let mut hints = std::collections::HashMap::new();
    if let Some(cat) = category {
        hints.insert("category", Value::from(cat));
    }
    let m = conn
        .call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &(app, 0u32, "", summary, body, acts, hints, -1i32),
        )
        .await
        .ok()?;
    let id: u32 = m.body().deserialize().ok()?;
    let signals = signals.as_mut()?;
    while let Some(msg) = signals.next().await {
        let Ok(msg) = msg else { continue };
        let h = msg.header();
        match h.member().map(|m| m.as_str()) {
            Some("ActionInvoked") => {
                if let Ok((nid, key)) = msg.body().deserialize::<(u32, String)>()
                    && nid == id
                {
                    return Some(key);
                }
            }
            Some("NotificationClosed")
                if msg
                    .body()
                    .deserialize::<(u32, u32)>()
                    .is_ok_and(|(nid, _)| nid == id) =>
            {
                return None;
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod run_notify_tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn run_is_none_on_a_failed_exit() {
        let sh = |s: &str| ["sh", "-c", s].map(String::from);
        assert_eq!(Live.run(&sh("echo ok"), None).as_deref(), Some("ok\n"));
        assert_eq!(Live.run(&sh("echo partial; exit 3"), None), None);
        assert_eq!(Live.run(&["ri-no-such-command".to_string()], None), None);
    }

    struct Daemon(std::process::Child, String);
    impl Drop for Daemon {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn daemon() -> Option<Daemon> {
        use std::io::BufRead;
        if !testenv::require("dbus-daemon") {
            return None;
        }
        let mut c = std::process::Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut addr = String::new();
        std::io::BufReader::new(c.stdout.take().unwrap())
            .read_line(&mut addr)
            .unwrap();
        Some(Daemon(c, addr.trim().to_string()))
    }

    fn connect(addr: &str) -> zbus::blocking::Connection {
        zbus::blocking::connection::Builder::address(addr)
            .unwrap()
            .build()
            .unwrap()
    }

    /// A notification server that reports the summary of every `Notify` and answers it with
    /// id 7 once the returned sender sends (or is dropped), then reports that it replied.
    fn server(
        addr: &str,
    ) -> (
        zbus::blocking::Connection,
        mpsc::Receiver<String>,
        mpsc::Sender<()>,
        mpsc::Receiver<()>,
    ) {
        let c = connect(addr);
        c.request_name("org.freedesktop.Notifications").unwrap();
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::MethodCall)
            .member("Notify")
            .unwrap()
            .build();
        let calls = zbus::blocking::MessageIterator::for_match_rule(rule, &c, None).unwrap();
        let (tx, rx) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let (replied_tx, replied) = mpsc::channel();
        let srv = c.clone();
        std::thread::spawn(move || {
            for m in calls.flatten() {
                type Notify<'a> = (
                    &'a str,
                    u32,
                    &'a str,
                    String,
                    &'a str,
                    Vec<&'a str>,
                    std::collections::HashMap<&'a str, zbus::zvariant::Value<'a>>,
                    i32,
                );
                let summary = m
                    .body()
                    .deserialize::<Notify>()
                    .map(|n| n.3)
                    .unwrap_or_default();
                if tx.send(summary).is_err() {
                    break;
                }
                let _ = released.recv();
                srv.reply(&m.header(), &7u32).unwrap();
                let _ = replied_tx.send(());
            }
        });
        (c, rx, release, replied)
    }

    fn signal<B: serde::Serialize + zbus::zvariant::DynamicType>(
        c: &zbus::blocking::Connection,
        member: &str,
        body: &B,
    ) {
        c.emit_signal(
            None::<&str>,
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
            member,
            body,
        )
        .unwrap();
    }

    fn show(
        client: &zbus::blocking::Connection,
        actions: &[(&str, &str)],
    ) -> mpsc::Receiver<Option<String>> {
        let (tx, rx) = mpsc::channel();
        notify_on(
            client.inner(),
            "app",
            None,
            "Upscale?",
            "body",
            actions,
            Box::new(move |k| {
                let _ = tx.send(k);
            }),
        );
        rx
    }

    #[test]
    fn notify_returns_before_the_user_acts() {
        let Some(d) = daemon() else { return };
        let (srv, shown, release, replied) = server(&d.1);
        let client = connect(&d.1);
        let (tx, returned) = mpsc::channel();
        let c = client.clone();
        std::thread::spawn(move || {
            let _ = tx.send(show(&c, &[("open_upscayl", "Open Upscayl")]));
        });
        // Returns while the server still holds its reply to Notify, and before the user acts:
        // a call that blocked on either never gets here (nextest's slow-timeout ends it).
        let chosen = returned.recv().expect("notify returned");
        assert_eq!(shown.recv().unwrap(), "Upscale?");
        assert_eq!(
            chosen.try_recv(),
            Err(mpsc::TryRecvError::Empty),
            "no answer before the reply"
        );
        release.send(()).unwrap();
        replied.recv().unwrap();
        // The answer comes once, so an early one (a dismissal, `None`) would fail the check of
        // the user's choice below.
        signal(&srv, "ActionInvoked", &(8u32, "other"));
        signal(&srv, "ActionInvoked", &(7u32, "open_upscayl"));
        assert_eq!(chosen.recv().unwrap().as_deref(), Some("open_upscayl"));
    }

    #[test]
    fn notify_reports_a_dismissal_as_none() {
        let Some(d) = daemon() else { return };
        let (srv, shown, release, replied) = server(&d.1);
        let client = connect(&d.1);
        let chosen = show(&client, &[("install_arch", "Install (Arch)")]);
        shown.recv().unwrap();
        release.send(()).unwrap();
        // The bus delivers the server's messages in order: the client sees the id before the
        // signal naming it.
        replied.recv().unwrap();
        signal(&srv, "NotificationClosed", &(7u32, 2u32));
        assert_eq!(chosen.recv().unwrap(), None);
    }

    #[test]
    fn notify_without_actions_answers_once_shown() {
        let Some(d) = daemon() else { return };
        let (_srv, shown, _, _) = server(&d.1);
        let client = connect(&d.1);
        let chosen = show(&client, &[]);
        shown.recv().unwrap();
        assert_eq!(chosen.recv().unwrap(), None);
    }
}
