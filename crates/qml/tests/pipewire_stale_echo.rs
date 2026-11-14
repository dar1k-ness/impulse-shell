//! A QML volume or mute write is shown until the server reported it: the snapshot the backend
//! publishes with the server's state from before a newer write (its echo of the previous
//! write) must not undo it and lose the next `Audio.incrementVolume()` step. Once the server
//! reported the last write, its value is shown again, also a change from another client.
//! Runs a private pipewire and no session manager: WirePlumber configures a new sink's ports
//! (PortConfig dsp) whenever it gets to it, and that reconfiguration resets its channelVolumes,
//! overwriting the writes this test checks (pipewire_reconfigure.rs covers the shell across
//! it). Fails when pipewire is not installed (`testenv`).
#![cfg(feature = "pipewire")]

use qml::pw::{NodeSnap, Snap, wait_snapshot_until};
use qml::runtime::Runtime;
use qml::value::Value;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixListener;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use testenv::Wakeups;

struct Kill(Vec<Child>);

impl Drop for Kill {
    fn drop(&mut self) {
        for c in self.0.iter_mut().rev() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn spawn(kids: &mut Kill, cmd: &mut Command) {
    let c = cmd
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    kids.0.push(c);
}

/// The sockets `pipewire.socket` lists, in its order.
const SOCKETS: [&str; 2] = ["pipewire-0", "pipewire-0-manager"];

/// `pipewire` socket-activated on [`SOCKETS`] in `dir` (sd_listen_fds(3), what the
/// `pipewire.socket` unit does): they are bound and listening before it starts, so clients
/// connect right away and their connections queue until the daemon accepts them.
fn pipewire(dir: &Path) -> Command {
    let fds: Vec<OwnedFd> = SOCKETS
        .iter()
        .map(|n| {
            let l = UnixListener::bind(dir.join(n)).unwrap();
            // Above the fds the child gets (3, 4, ...), so no dup2 there overwrites another.
            // SAFETY: F_DUPFD_CLOEXEC returns a new fd this process owns, or -1.
            let fd = unsafe {
                libc::fcntl(
                    l.as_raw_fd(),
                    libc::F_DUPFD_CLOEXEC,
                    3 + SOCKETS.len() as i32,
                )
            };
            assert!(fd >= 0, "{}", std::io::Error::last_os_error());
            // SAFETY: as above.
            unsafe { OwnedFd::from_raw_fd(fd) }
        })
        .collect();
    let mut c = Command::new("sh");
    // LISTEN_PID is the daemon's own pid, which only the exec'ing shell knows.
    c.args(["-c", "LISTEN_PID=$$ exec pipewire"])
        .env("LISTEN_FDS", SOCKETS.len().to_string())
        .env("LISTEN_FDNAMES", SOCKETS.join(":"));
    let dup = move || {
        for (i, fd) in fds.iter().enumerate() {
            // SAFETY: dup2 touches only the child's fd table; the new fd does not carry
            // FD_CLOEXEC, so it survives both execs.
            if unsafe { libc::dup2(fd.as_raw_fd(), 3 + i as i32) } < 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
        Ok(())
    };
    // SAFETY: `dup` runs in the forked child before exec and only calls dup2, which is
    // async-signal-safe.
    unsafe { c.pre_exec(dup) };
    c
}

const QML: &str = r#"import QtQuick
import Rustshell.Services.Pipewire
Item {
    readonly property var sink: Pipewire.nodes.values.find(n => n.name === "regecho") ?? null
    readonly property bool ready: sink?.ready ?? false
    PwObjectTracker { objects: [sink] }
}
"#;

/// The sink once a snapshot shows it as `get` wants, woken by each snapshot the backend
/// publishes (the runtime does not see it until its next dispatch).
fn sink(what: &str, get: impl Fn(&NodeSnap) -> bool) -> NodeSnap {
    // Named in the output nextest shows when its slow-timeout ends a hang.
    eprintln!("waiting for {what}");
    let find = |s: &Snap| {
        s.nodes
            .iter()
            .find(|n| n.name == "regecho" && get(n))
            .map(|n| (**n).clone())
    };
    find(&wait_snapshot_until(|s| find(s).is_some())).unwrap()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-4
}

#[test]
fn a_stale_echo_does_not_revert_a_newer_write() {
    const TEST: &str = concat!(
        module_path!(),
        "::a_stale_echo_does_not_revert_a_newer_write"
    );
    // libpipewire reads the environment with getenv(3), so the test runs in a process of its
    // own with a private session in a fresh directory and no outer `PIPEWIRE_REMOTE`.
    if !testenv::in_own_process(TEST) {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        std::fs::create_dir_all(d.join("state")).unwrap();
        let mut bus = std::ffi::OsString::from("unix:path=");
        bus.push(d.join("bus"));
        testenv::run_in_own_process_env(
            TEST,
            [
                ("XDG_RUNTIME_DIR", d.into()),
                ("XDG_STATE_HOME", d.join("state").into()),
                ("DBUS_SESSION_BUS_ADDRESS", bus),
            ],
            &["PIPEWIRE_REMOTE"],
        );
        return;
    }
    let dir = std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap());
    let bus = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    if !testenv::require_all(&["dbus-daemon", "pipewire", "pw-cli"]) {
        return;
    }
    let mut kids = Kill(vec![]);
    let addr = format!("--address={bus}");
    spawn(
        &mut kids,
        Command::new("dbus-daemon").args(["--session", "--nofork", "--nopidfile", &addr]),
    );
    spawn(&mut kids, &mut pipewire(&dir));
    let mut rt = Runtime::new(None);
    let wake = Wakeups::new();
    rt.world.borrow().waker.set_ping(wake.ping());
    let ep = rt.process_fd();
    let root = rt.load_str(QML).unwrap();
    rt.settle();
    let node = "{ factory.name=support.null-audio-sink media.class=Audio/Sink node.name=regecho audio.position=[FL,FR] object.linger=true }";
    let st = Command::new("pw-cli")
        .args(["create-node", "adapter", node])
        .output()
        .unwrap();
    assert!(st.status.success(), "pw-cli: {st:?}");
    eprintln!("waiting for the sink to become ready");
    loop {
        rt.poll_native();
        rt.settle();
        if rt.get(root, "ready").truthy() {
            break;
        }
        if wake.wait(ep, None).fd {
            rt.process_fd_ready();
        }
    }
    let audio = {
        let mut w = rt.world.borrow_mut();
        let Some(Value::Item(n)) = w.read(root, "sink") else {
            panic!("no sink")
        };
        let Some(Value::Item(a)) = w.read(n, "audio") else {
            panic!("no audio")
        };
        a
    };
    let read = |rt: &Runtime, p: &str| rt.world.borrow_mut().read(audio, p).unwrap();
    let write = |rt: &mut Runtime, p: &str, v: Value| {
        rt.world.borrow_mut().write(audio, p, v);
        rt.poll_native();
        rt.settle();
    };
    let shown = |rt: &Runtime| read(rt, "volume").to_number();
    let dispatch = |rt: &mut Runtime| {
        rt.poll_native();
        rt.settle();
    };

    // Write ids count this runtime's writes from 1.
    write(&mut rt, "volume", Value::Num(0.6));
    // The server reported 0.6; the runtime has not seen that snapshot yet when it takes the
    // next step.
    sink("the first volume write", |n| {
        n.volumes_ack >= 1 && close(n.volumes[0] as f64, 0.6)
    });
    let v = shown(&rt) + 0.1;
    write(&mut rt, "volume", Value::Num(v));
    assert!(close(shown(&rt), 0.7), "reverted to {}", shown(&rt));
    sink("the second volume write", |n| n.volumes_ack >= 2);
    dispatch(&mut rt);
    let v = shown(&rt) + 0.1;
    write(&mut rt, "volume", Value::Num(v));
    let end = sink("the third volume write", |n| n.volumes_ack >= 3);
    dispatch(&mut rt);
    assert!(
        close(end.volumes[0] as f64, 0.8),
        "the server has {:?}: a step was lost",
        end.volumes
    );
    assert!(close(shown(&rt), 0.8), "shown {}", shown(&rt));

    // Two quick toggles: the echo of the first does not flip the switch back.
    write(&mut rt, "muted", Value::Bool(true));
    sink("the first mute write", |n| n.muted_ack >= 4 && n.muted);
    write(&mut rt, "muted", Value::Bool(false));
    assert!(
        !read(&rt, "muted").truthy(),
        "the echo of the first toggle muted it"
    );
    let end = sink("the second mute write", |n| n.muted_ack >= 5);
    dispatch(&mut rt);
    assert!(!end.muted);
    assert!(!read(&rt, "muted").truthy());

    // Another client's change is shown: nothing the shell wrote is pending.
    let st = Command::new("pw-cli")
        .args([
            "set-param",
            &end.id.to_string(),
            "Props",
            "{ channelVolumes = [ 0.125, 0.125 ] }",
        ])
        .output()
        .unwrap();
    assert!(st.status.success(), "pw-cli: {st:?}");
    sink("pw-cli's volume", |n| close(n.volumes[0] as f64, 0.5));
    dispatch(&mut rt);
    assert!(close(shown(&rt), 0.5), "shown {}", shown(&rt));
    drop(kids);
}
