//! WirePlumber configures a new sink's ports when it activates it (PortConfig `dsp`, see
//! `si-audio-adapter`), and that reconfiguration of the adapter resets its channelVolumes: a
//! volume set before is gone on the server. The shell must show the node's new volume, write
//! nothing back on its own (no stale value, nothing above 100%), and its next slider step must
//! start from the new volume and land. Runs a private pipewire, then wireplumber once the
//! shell's write was acked, so the reconfiguration comes after it; fails when they are not
//! installed (`testenv`).
#![cfg(feature = "pipewire")]

use qml::pw::{NodeSnap, Snap, wait_snapshot_until};
use qml::runtime::Runtime;
use qml::value::Value;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixListener;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use testenv::{Log, Wakeups};

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
    readonly property var sink: Pipewire.nodes.values.find(n => n.name === "regcfg") ?? null
    readonly property bool ready: sink?.ready ?? false
    PwObjectTracker { objects: [sink] }
}
"#;

/// The sink once a snapshot the backend published shows it as `get` wants.
fn sink(what: &str, get: impl Fn(&NodeSnap) -> bool) -> NodeSnap {
    // Named in the output nextest shows when its slow-timeout ends a hang.
    eprintln!("waiting for {what}");
    let find = |s: &Snap| {
        s.nodes
            .iter()
            .find(|n| n.name == "regcfg" && get(n))
            .map(|n| (**n).clone())
    };
    find(&wait_snapshot_until(|s| find(s).is_some())).unwrap()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-4
}

/// The node's Props `channelVolumes` (linear) on the server, from `pw-dump`.
fn server_volumes(id: u32) -> Vec<f64> {
    let out = Command::new("pw-dump")
        .arg(id.to_string())
        .output()
        .unwrap();
    assert!(out.status.success(), "pw-dump: {out:?}");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    // pw-dump also prints an object whose `object.serial` is `id`, before the node if its
    // own id is lower.
    v.as_array()
        .and_then(|a| a.iter().find(|o| o["id"].as_u64() == Some(id.into())))
        .and_then(|o| o["info"]["params"]["Props"][0]["channelVolumes"].as_array())
        .unwrap_or_else(|| panic!("no channelVolumes: {v}"))
        .iter()
        .map(|x| x.as_f64().unwrap())
        .collect()
}

#[test]
fn a_session_manager_reconfiguring_the_sink_is_followed() {
    const TEST: &str = concat!(
        module_path!(),
        "::a_session_manager_reconfiguring_the_sink_is_followed"
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
    let tools = [
        "dbus-daemon",
        "pipewire",
        "wireplumber",
        "pw-cli",
        "pw-dump",
        "pw-link",
    ];
    if !testenv::require_all(&tools) {
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
    // Like the shell's loop: poll again while a poll's own writes changed the world, then sleep
    // until the runtime is woken.
    let until = |rt: &mut Runtime, what: &str, done: &dyn Fn(&mut Runtime) -> bool| {
        eprintln!("waiting for {what}");
        loop {
            loop {
                let rev = rt.revision();
                rt.poll_native();
                rt.settle();
                if rt.revision() == rev {
                    break;
                }
            }
            if done(rt) {
                return;
            }
            if wake.wait(ep, None).fd {
                rt.process_fd_ready();
            }
        }
    };
    let node = "{ factory.name=support.null-audio-sink media.class=Audio/Sink node.name=regcfg audio.position=[FL,FR] object.linger=true }";
    let st = Command::new("pw-cli")
        .args(["create-node", "adapter", node])
        .output()
        .unwrap();
    assert!(st.status.success(), "pw-cli: {st:?}");
    until(&mut rt, "the sink to become ready", &|rt| {
        rt.get(root, "ready").truthy()
    });
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
    let shown = move |rt: &Runtime| {
        rt.world
            .borrow_mut()
            .read(audio, "volume")
            .unwrap()
            .to_number()
    };
    let write = |rt: &mut Runtime, p: &str, v: Value| {
        rt.world.borrow_mut().write(audio, p, v);
        rt.poll_native();
        rt.settle();
    };

    // Write ids count this runtime's writes from 1.
    write(&mut rt, "volume", Value::Num(0.6));
    let before = sink("the volume write", |n| {
        n.volumes_ack >= 1 && close(n.volumes[0] as f64, 0.6)
    });

    // The session manager activates the sink: its dsp ports appear (pw-link prints each one
    // as it is added), and the adapter's reconfiguration resets the volumes it had.
    let mut mon = Command::new("pw-link")
        .args(["--monitor", "--output"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let ports = Log::capture(mon.stdout.take().unwrap());
    kids.0.push(mon);
    spawn(&mut kids, &mut Command::new("wireplumber"));
    eprintln!("waiting for the sink's dsp ports");
    ports
        .wait(|t| t.lines().any(|l| l.ends_with("regcfg:monitor_FR")))
        .expect("pw-link exited before the sink's ports appeared");
    until(&mut rt, "the reset volume to be shown", &|rt| {
        close(shown(rt), 1.0)
    });
    let server = server_volumes(before.id);
    assert!(
        server.iter().all(|v| close(*v, 1.0)),
        "the server has {server:?} after the reconfiguration"
    );
    let now = sink("the reset volume", |n| close(n.volumes[0] as f64, 1.0));
    assert_eq!(
        now.volumes_ack, 1,
        "the shell wrote the volume back on its own"
    );

    // A slider step starts from the volume shown, and lands.
    let v = shown(&rt) - 0.1;
    write(&mut rt, "volume", Value::Num(v));
    let end = sink("the step after the reconfiguration", |n| n.volumes_ack >= 2);
    assert_eq!(end.volumes_ack, 2, "an extra write went out: {end:?}");
    assert!(
        end.volumes.iter().all(|x| close(*x as f64, 0.9)),
        "the server has {:?}, not the step from the new volume",
        end.volumes
    );
    let server = server_volumes(end.id);
    assert!(
        server.iter().all(|x| close(x.cbrt(), 0.9)),
        "the server has {server:?}"
    );
    until(&mut rt, "the step to be shown", &|rt| close(shown(rt), 0.9));

    // Mute still reaches the server.
    write(&mut rt, "muted", Value::Bool(true));
    let end = sink("the mute write", |n| n.muted_ack >= 3);
    assert!(end.muted, "the mute write did not land: {end:?}");
    drop(kids);
}
