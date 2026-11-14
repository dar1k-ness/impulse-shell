//! A node turns `ready` only after the core sync that follows its first info and Props
//! enumeration (quickshell `PwNode::onCoreSync`), so its initial volume is already set: the original's
//! OnScreenDisplay shows on volume changes of a ready sink, and must not pop at startup.
//! Runs a private pipewire + wireplumber; fails when they are not installed (`testenv`).
#![cfg(feature = "pipewire")]

use qml::runtime::Runtime;
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
import Rustshell
import Rustshell.Services.Pipewire
Item {
    id: root
    readonly property var sink: Pipewire.nodes.values.find(n => n.name === "regready") ?? null
    readonly property bool ready: sink?.ready ?? false
    property real volAtReady: -1
    property int changesWhileReady: 0
    onReadyChanged: if (ready && volAtReady < 0) volAtReady = sink.audio.volume
    PwObjectTracker { objects: [root.sink] }
    Connections {
        target: root.sink?.audio ?? null
        function onVolumeChanged() { if (root.ready) root.changesWhileReady++; }
    }
}
"#;

#[test]
fn sink_is_ready_with_its_initial_volume() {
    const TEST: &str = concat!(module_path!(), "::sink_is_ready_with_its_initial_volume");
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
                ("PIPEWIRE_RUNTIME_DIR", d.into()),
                ("XDG_STATE_HOME", d.join("state").into()),
                ("DBUS_SESSION_BUS_ADDRESS", bus),
            ],
            &["PIPEWIRE_REMOTE"],
        );
        return;
    }
    let dir = std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap());
    let bus = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    if !testenv::require_all(&["dbus-daemon", "pipewire", "wireplumber", "pw-cli"]) {
        return;
    }
    let mut kids = Kill(vec![]);
    let addr = format!("--address={bus}");
    spawn(
        &mut kids,
        Command::new("dbus-daemon").args(["--session", "--nofork", "--nopidfile", &addr]),
    );
    spawn(&mut kids, &mut pipewire(&dir));
    spawn(&mut kids, &mut Command::new("wireplumber"));
    let mut rt = Runtime::new(None);
    let wake = Wakeups::new();
    rt.world.borrow().waker.set_ping(wake.ping());
    let ep = rt.process_fd();
    let root = rt.load_str(QML).unwrap();
    rt.settle();
    // The sink appears while the shell is connected, as on a hotplug or a late session start.
    let sink = "{ factory.name=support.null-audio-sink media.class=Audio/Sink node.name=regready audio.position=[FL,FR] object.linger=true }";
    let st = Command::new("pw-cli")
        .args(["create-node", "adapter", sink])
        .output()
        .unwrap();
    assert!(st.status.success(), "pw-cli: {st:?}");
    // Named in the output nextest shows when its slow-timeout ends a hang.
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
    assert_eq!(rt.get(root, "volAtReady").to_number(), 1.0);
    assert_eq!(rt.get(root, "changesWhileReady").to_number(), 0.0);
    // Nodes carry the registry global's `object.serial`.
    let serial = qml::pw::snapshot().and_then(|s| {
        s.nodes
            .iter()
            .find(|n| n.name == "regready")
            .and_then(|n| n.serial)
    });
    assert!(
        serial.is_some_and(|s| s > 0),
        "no object.serial: {serial:?}"
    );
    drop(kids);
    let _ = std::fs::remove_dir_all(&dir);
}
