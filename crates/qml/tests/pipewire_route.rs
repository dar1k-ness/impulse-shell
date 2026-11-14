//! Volume and mute of a sink backed by a card's Route (an ALSA sink as WirePlumber exports it)
//! go to that device's Route, not to the node's Props, and the server's state of the Route is
//! what the sink shows: the echo of a QML write and a change made by someone else. The card
//! is emulated by a client-exported SPA device (`support/route_device.rs`).
//! Runs a private pipewire + wireplumber; fails when they are not installed (`testenv`).
#![cfg(feature = "pipewire")]

#[path = "support/route_device.rs"]
mod route_device;

use qml::pw::{NodeSnap, Snap, wait_snapshot_until};
use qml::runtime::Runtime;
use qml::value::Value;
use route_device::{RouteDevice, create_sink};
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
    readonly property var sink: Pipewire.nodes.values.find(n => n.name === "routesink") ?? null
    readonly property bool ready: sink?.ready ?? false
    readonly property real leftVolume: sink?.audio?.volumes[0] ?? -1
    readonly property real rightVolume: sink?.audio?.volumes[1] ?? -1
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
            .find(|n| n.name == "routesink" && get(n))
            .map(|n| (**n).clone())
    };
    find(&wait_snapshot_until(|s| find(s).is_some())).unwrap()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-4
}

fn all_close(v: &[f32], want: &[f64]) -> bool {
    v.len() == want.len() && v.iter().zip(want).all(|(a, b)| close(*a as f64, *b))
}

/// The node's own `channelVolumes` (its Props param) as the server has them.
fn node_volumes(id: u32) -> Vec<f64> {
    let out = Command::new("pw-dump")
        .arg(id.to_string())
        .output()
        .unwrap();
    assert!(out.status.success(), "pw-dump: {out:?}");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    v.as_array()
        .and_then(|a| a.iter().find(|o| o["id"].as_u64() == Some(id.into())))
        .and_then(|o| o["info"]["params"]["Props"].as_array())
        .and_then(|p| p.iter().find_map(|p| p["channelVolumes"].as_array()))
        .unwrap_or_else(|| panic!("no Props channelVolumes for node {id}"))
        .iter()
        .map(|x| x.as_f64().unwrap())
        .collect()
}

#[test]
fn route_backed_sink_volume_and_mute_go_through_the_route() {
    const TEST: &str = concat!(
        module_path!(),
        "::route_backed_sink_volume_and_mute_go_through_the_route"
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
                ("XDG_CONFIG_HOME", d.join("config").into()),
                ("DBUS_SESSION_BUS_ADDRESS", bus),
            ],
            &["PIPEWIRE_REMOTE"],
        );
        return;
    }
    let dir = std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap());
    let bus = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    if !testenv::require_all(&[
        "dbus-daemon",
        "pipewire",
        "wireplumber",
        "pw-cli",
        "pw-dump",
    ]) {
        return;
    }
    let mut kids = Kill(vec![]);
    let addr = format!("--address={bus}");
    spawn(
        &mut kids,
        Command::new("dbus-daemon").args(["--session", "--nofork", "--nopidfile", &addr]),
    );
    spawn(&mut kids, &mut pipewire(&dir));
    route_device::write_wireplumber_conf(&dir.join("config"));
    spawn(&mut kids, &mut Command::new("wireplumber"));
    let card = RouteDevice::start("routecard", &[0.8, 0.8]);
    let mut rt = Runtime::new(None);
    let wake = Wakeups::new();
    rt.world.borrow().waker.set_ping(wake.ping());
    let ep = rt.process_fd();
    let root = rt.load_str(QML).unwrap();
    rt.settle();
    create_sink("routesink", card.id());
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
    let channels = |rt: &Runtime| {
        [
            rt.get(root, "leftVolume").to_number(),
            rt.get(root, "rightVolume").to_number(),
        ]
    };
    let dispatch = |rt: &mut Runtime| {
        rt.poll_native();
        rt.settle();
    };

    // The sink shows its Route, not the node's own volume (1.0 for a new null sink).
    let n = sink("the route's volume", |n| all_close(&n.volumes, &[0.8, 0.8]));
    dispatch(&mut rt);
    assert!(close(shown(&rt), 0.8), "shown {}", shown(&rt));
    let node = n.id;
    assert_eq!(node_volumes(node), [1.0, 1.0]);

    // Write ids count this runtime's writes from 1.
    write(&mut rt, "volume", Value::Num(0.5));
    let set = card.wait_set("volume 0.5", |s| {
        s.visual_volumes.iter().all(|v| close(*v as f64, 0.5))
    });
    assert!(!set.muted);
    // The first snapshot after the server reported the write shows the Route's new state.
    let n = sink("the volume write's ack", |n| n.volumes_ack >= 1);
    assert!(
        all_close(&n.volumes, &[0.5, 0.5]),
        "acked with {:?}",
        n.volumes
    );
    dispatch(&mut rt);
    assert!(close(shown(&rt), 0.5), "shown {}", shown(&rt));
    assert_eq!(node_volumes(node), [1.0, 1.0], "the write went to the node");

    write(&mut rt, "muted", Value::Bool(true));
    let set = card.wait_set("mute", |s| s.muted);
    assert!(
        all_close(&set.visual_volumes, &[0.5, 0.5]),
        "mute changed the volume: {set:?}"
    );
    let n = sink("the mute write's ack", |n| n.muted_ack >= 2);
    assert!(n.muted, "acked unmuted");
    dispatch(&mut rt);
    assert!(read(&rt, "muted").truthy());

    // A change of the Route by the hardware or another client is shown.
    card.set_external(&[0.3, 0.6]);
    sink("the external route change", |n| {
        all_close(&n.volumes, &[0.3, 0.6])
    });
    dispatch(&mut rt);
    let [l, r] = channels(&rt);
    assert!(close(l, 0.3) && close(r, 0.6), "shown channels {l} {r}");
    assert!(close(shown(&rt), 0.45), "shown {}", shown(&rt));
    assert!(read(&rt, "muted").truthy());
    drop(rt);
    drop(card);
    drop(kids);
}
