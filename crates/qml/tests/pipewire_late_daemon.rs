//! A shell started before PipeWire must pick it up as soon as the daemon appears, not after a
//! retry backoff (it was 1, 2, 4 ... 30 s apart). Runs a private pipewire + wireplumber; fails
//! when they are not installed (`testenv`).
#![cfg(feature = "pipewire")]

use qml::runtime::Runtime;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use testenv::Wakeups;
use testenv::pipewire::socket_activated_pipewire;

struct Kill(Vec<Child>);

impl Drop for Kill {
    fn drop(&mut self) {
        for c in self.0.iter_mut().rev() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// The environment of a private PipeWire session in `dir` (made here) for the test's own
/// process (`testenv::run_in_own_process`): runtime and state directories and session bus in
/// `dir`. The process runs without `PIPEWIRE_REMOTE` ([`UNSET`]), so an outer one cannot point
/// the shell, its daemons or tools elsewhere.
fn private_session(dir: &Path) -> Vec<(&'static str, OsString)> {
    std::fs::create_dir_all(dir.join("state")).unwrap();
    let mut bus = OsString::from("unix:path=");
    bus.push(dir.join("bus"));
    vec![
        ("XDG_RUNTIME_DIR", dir.into()),
        ("PIPEWIRE_RUNTIME_DIR", dir.into()),
        ("XDG_STATE_HOME", dir.join("state").into()),
        ("DBUS_SESSION_BUS_ADDRESS", bus),
    ]
}

/// What the test's own process runs without.
const UNSET: &[&str] = &["PIPEWIRE_REMOTE"];

fn spawn(kids: &mut Kill, cmd: &str, args: &[&str]) -> Option<()> {
    spawn_cmd(kids, Command::new(cmd).args(args), cmd)
}

fn spawn_cmd(kids: &mut Kill, c: &mut Command, cmd: &str) -> Option<()> {
    let c = c
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| testenv::unavailable(format!("no {cmd} ({e})")))
        .ok()?;
    kids.0.push(c);
    Some(())
}

/// Runs the shell's events until `done`, blocking on its wakeups in between (a hang is
/// nextest's slow-timeout to end).
fn run_until(
    rt: &mut Runtime,
    wake: &Wakeups,
    ep: Option<i32>,
    done: impl Fn(&mut Runtime) -> bool,
) {
    loop {
        rt.poll_native();
        rt.settle();
        if done(rt) {
            return;
        }
        if wake.wait(ep, None).fd {
            rt.process_fd_ready();
        }
    }
}

const QML: &str = r#"import QtQuick
import Rustshell
import Rustshell.Services.Pipewire
Item {
    readonly property bool seen: Pipewire.nodes.values.some(n => n.name === "regnul")
}
"#;

/// In its own process with a private session (`private_session`).
#[test]
fn daemon_started_after_the_shell_is_picked_up_at_once() {
    const TEST: &str = concat!(
        module_path!(),
        "::daemon_started_after_the_shell_is_picked_up_at_once"
    );
    if !testenv::in_own_process(TEST) {
        let dir = tempfile::tempdir().unwrap();
        testenv::run_in_own_process_env(TEST, private_session(dir.path()), UNSET);
        return;
    }
    let dir = PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap());
    let bus = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    let mut kids = Kill(vec![]);
    let addr = format!("--address={bus}");
    if spawn(
        &mut kids,
        "dbus-daemon",
        &["--session", "--nofork", "--nopidfile", &addr],
    )
    .is_none()
    {
        return;
    }
    let mut rt = Runtime::new(None);
    let wake = Wakeups::new();
    rt.world.borrow().waker.set_ping(wake.ping());
    let ep = rt.process_fd();
    let root = rt.load_str(QML).unwrap();
    rt.settle();
    // The shell's first attempt failed (no socket): it published the disconnected state and
    // now waits for the socket, where the old backoff slept 1, 2, 4 ... 30 s.
    assert!(!qml::pw::wait_snapshot().connected);
    // Binding the socket is what puts it in the runtime directory.
    if spawn_cmd(&mut kids, &mut socket_activated_pipewire(&dir), "pipewire").is_none()
        || spawn(&mut kids, "wireplumber", &[]).is_none()
    {
        return;
    }
    let sink = "{ factory.name=support.null-audio-sink media.class=Audio/Sink node.name=regnul audio.position=[FL,FR] object.linger=true }";
    let Ok(st) = Command::new("pw-cli")
        .args(["create-node", "adapter", sink])
        .output()
    else {
        testenv::unavailable("no pw-cli");
        return;
    };
    assert!(st.status.success());
    // With no retry timer left in the shell (`pw` unit test
    // `without_socket_events_a_failed_attempt_is_final`), only the socket's appearance can
    // have connected it.
    run_until(&mut rt, &wake, ep, |rt| rt.get(root, "seen").truthy());
}
