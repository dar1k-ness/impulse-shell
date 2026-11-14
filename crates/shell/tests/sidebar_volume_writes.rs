//! The tree's right sidebar sound controls write through to PipeWire: the QuickSliders volume
//! slider and a VolumeMixerEntry, dragged with the pointer, set the volume the server reports
//! for the node they show; after WirePlumber switches the default sink (`wpctl set-default`,
//! its `default.configured.audio.sink` metadata) the quick slider writes to the new one. A
//! volume above 100%, the shell's or another client's, reaches the server and stays, as with
//! quickshell; with the original's protection on, "Volume limit" (150%) is where it is brought back.
//! Unbalanced channels keep their ratio. Runs a private pipewire + wireplumber; fails when
//! they are not installed (`testenv`).

#[path = "../../qml/tests/support/route_device.rs"]
mod route_device;

use qml::pw::{NodeSnap, Snap, wait_snapshot_until};
use qml::runtime::Runtime;
use qml::value::Value;
use route_device::{RouteDevice, create_sink};
use shell::{input::Pointer, scene};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixListener;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use testenv::Wakeups;

const BTN_LEFT: u32 = 0x110;
const SIZE: [f64; 2] = [460.0, 400.0];

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
/// `pipewire.socket` unit does): clients connect right away and queue until it accepts.
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

const MAIN: &str = r#"import QtQuick
import ri.modules.common
import ri.services
import ri.modules.rusty.sidebarRight
import ri.modules.rusty.sidebarRight.volumeMixer
Item {
    width: 460
    height: 400
    property alias sliders: sliders
    property alias entry: entry
    readonly property string sinkName: Audio.sink?.name ?? ""
    readonly property bool sinkReady: Audio.sink?.ready ?? false
    readonly property real sinkVolume: Audio.sink?.audio.volume ?? -1
    readonly property var app: Audio.outputAppNodes.find(n => n.name === "mixstream") ?? null
    readonly property bool appReady: app?.ready ?? false
    property int protections: 0
    Connections {
        target: Audio
        function onSinkProtectionTriggered(reason) { protections++ }
    }
    function setSinkVolume(v) { Audio.sink.audio.volume = v }
    function protect(on, limit) {
        Config.options.audio.protection.maxAllowed = limit
        Config.options.audio.protection.enable = on
    }
    QuickSliders { id: sliders; width: 460 }
    VolumeMixerEntry { id: entry; y: 200; width: 460; node: app }
}
"#;

fn tree() -> PathBuf {
    let ri = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qml")
        .canonicalize()
        .unwrap();
    let d = std::env::temp_dir().join(format!("sidebar-volume-writes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    for e in std::fs::read_dir(&ri).unwrap() {
        let e = e.unwrap();
        std::os::unix::fs::symlink(e.path(), d.join(e.file_name())).unwrap();
    }
    std::fs::write(d.join("zzmain.qml"), MAIN).unwrap();
    d
}

fn pw_cli(args: &[&str]) -> String {
    let st = Command::new("pw-cli").args(args).output().unwrap();
    assert!(st.status.success(), "pw-cli {args:?}: {st:?}");
    String::from_utf8_lossy(&st.stdout).into_owned()
}

/// A null sink (or, with `class`, a stream) named `name` with channels FL, FR.
/// `state.restore-props=false`: WirePlumber sets a new node's volume once, to its stored or
/// default one (`node/restore-stream`), at a moment no client can wait for, and so over a
/// write made just before; with the property it leaves the node's volume alone.
fn create_node(name: &str, class: &str) {
    pw_cli(&[
        "create-node",
        "adapter",
        &format!(
            "{{ factory.name=support.null-audio-sink media.class={class} node.name={name} \
             application.name={name} audio.position=[FL,FR] object.linger=true \
             state.restore-props=false }}"
        ),
    ]);
}

/// The node `name` once a backend snapshot shows it as `get` wants, woken by each snapshot.
fn server(name: &str, what: &str, get: impl Fn(&NodeSnap) -> bool) -> NodeSnap {
    // Named in the output nextest shows when its slow-timeout ends a hang.
    eprintln!("waiting for {what}");
    let find = |s: &Snap| {
        s.nodes
            .iter()
            .find(|n| n.name == name && get(n))
            .map(|n| (**n).clone())
    };
    find(&wait_snapshot_until(|s| find(s).is_some())).unwrap()
}

fn wpctl_set_volume(id: &str, volume: &str) {
    let st = Command::new("wpctl")
        .args(["set-volume", id, volume])
        .status()
        .unwrap();
    assert!(st.success(), "wpctl set-volume {id} {volume}: {st:?}");
}

/// What `wpctl get-volume` prints for node `id`: the volume as the server holds it.
fn wpctl_get_volume(id: &str) -> String {
    let out = Command::new("wpctl")
        .args(["get-volume", id])
        .output()
        .unwrap();
    assert!(out.status.success(), "wpctl get-volume {id}: {out:?}");
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn close(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3)
}

struct Shell {
    rt: Runtime,
    wake: Wakeups,
    ep: Option<std::os::fd::RawFd>,
    main: usize,
    fonts: scene::Fonts,
}

impl Shell {
    /// Dispatches the runtime's events until `done` holds for its root.
    fn until(&mut self, what: &str, done: impl Fn(&mut Runtime, usize) -> bool) {
        eprintln!("waiting for {what}");
        loop {
            self.rt.poll_native();
            self.rt.settle();
            if done(&mut self.rt, self.main) {
                return;
            }
            if self.wake.wait(self.ep, None).fd {
                self.rt.process_fd_ready();
            }
        }
    }

    /// Calls the root's function `name`.
    fn call(&mut self, name: &str, args: Vec<Value>) {
        let r = self.rt.call_method(self.main, name, args);
        r.unwrap_or_else(|| panic!("no {name}"))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        self.rt.poll_native();
        self.rt.settle();
    }

    /// `Audio.sink.audio.volume = v`, as QML writes it.
    fn set_sink_volume(&mut self, v: f64) {
        self.call("setSinkVolume", vec![Value::Num(v)]);
    }

    fn item(&mut self, name: &str) -> usize {
        match self.rt.world.borrow_mut().read(self.main, name) {
            Some(Value::Item(i)) => i,
            v => panic!("{name}: {v:?}"),
        }
    }

    /// The live StyledSlider under `under` for which `pick` holds.
    fn slider(
        &mut self,
        under: usize,
        pick: impl Fn(&mut qml::runtime::World, usize) -> bool,
    ) -> usize {
        let mut w = self.rt.world.borrow_mut();
        let found = (0..w.items.len()).find(|&i| {
            if w.dead.contains(&i) || !w.items[i].types.iter().any(|t| t == "StyledSlider") {
                return false;
            }
            let mut p = Some(i);
            while let Some(q) = p {
                if q == under {
                    return pick(&mut w, i);
                }
                p = w.items[q].parent;
            }
            false
        });
        found.expect("no slider")
    }

    /// Presses `slider` at its left end, drags to `at` of its width and releases, as a user
    /// does; returns the value the slider moved to.
    fn drag(&mut self, slider: usize, at: f64) -> f64 {
        scene::layout_window(&mut self.rt, &mut self.fonts, self.main, SIZE);
        self.rt.settle();
        let (x, y, wd, h) = {
            let mut w = self.rt.world.borrow_mut();
            let (x, y) = w.scene_pos(slider, Some(self.main));
            let num = |w: &mut qml::runtime::World, p: &str| w.read(slider, p).unwrap().to_number();
            (x, y, num(&mut w, "width"), num(&mut w, "height"))
        };
        assert!(wd > 0.0 && h > 0.0, "slider not laid out: {wd}x{h}");
        let cy = y + h / 2.0;
        let mut ptr = Pointer::default();
        ptr.enter(&mut self.rt, self.main, x + 1.0, cy);
        ptr.motion(&mut self.rt, self.main, x + 1.0, cy);
        ptr.press(&mut self.rt, BTN_LEFT);
        ptr.motion(&mut self.rt, self.main, x + wd * at, cy);
        ptr.release(&mut self.rt, BTN_LEFT);
        self.rt.poll_native();
        self.rt.settle();
        self.rt
            .world
            .borrow_mut()
            .read(slider, "value")
            .unwrap()
            .to_number()
    }
}

#[test]
fn sidebar_sound_sliders_write_through() {
    const TEST: &str = concat!(module_path!(), "::sidebar_sound_sliders_write_through");
    // libpipewire reads the environment with getenv(3), so the test runs in a process of its
    // own with a private session in a fresh directory and no outer `PIPEWIRE_REMOTE`.
    if !testenv::in_own_process(TEST) {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        std::fs::create_dir_all(d.join("state")).unwrap();
        std::fs::create_dir_all(d.join("home")).unwrap();
        let mut bus = std::ffi::OsString::from("unix:path=");
        bus.push(d.join("bus"));
        testenv::run_in_own_process_env(
            TEST,
            [
                ("XDG_RUNTIME_DIR", d.into()),
                ("XDG_STATE_HOME", d.join("state").into()),
                ("XDG_CONFIG_HOME", d.join("home").into()),
                ("DBUS_SESSION_BUS_ADDRESS", bus),
            ],
            &["PIPEWIRE_REMOTE"],
        );
        return;
    }
    let dir = PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap());
    let bus = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    if !testenv::require_all(&["dbus-daemon", "pipewire", "wireplumber", "pw-cli", "wpctl"]) {
        return;
    }
    let mut kids = Kill(vec![]);
    let addr = format!("--address={bus}");
    spawn(
        &mut kids,
        Command::new("dbus-daemon").args(["--session", "--nofork", "--nopidfile", &addr]),
    );
    spawn(&mut kids, &mut pipewire(&dir));
    route_device::write_wireplumber_conf(&PathBuf::from(
        std::env::var_os("XDG_CONFIG_HOME").unwrap(),
    ));
    spawn(&mut kids, &mut Command::new("wireplumber"));

    let d = tree();
    // Midday for the engine: the original's night light schedule (Hyprsunset, loaded with the services)
    // decides nothing here from the hour the test runs at.
    qml::builtins::set_clock(Some(testenv::local_time_ms(12, 0)));
    let mut rt = Runtime::new(Some(d.clone()));
    let wake = Wakeups::new();
    rt.world.borrow().waker.set_ping(wake.ping());
    let ep = rt.process_fd();
    rt.load_singletons(&[
        d.clone(),
        d.join("modules/common"),
        d.join("modules/common/functions"),
        d.join("services"),
    ]);
    rt.settle();
    let main = rt.load_file(&d.join("zzmain.qml")).unwrap();
    rt.settle();
    let mut sh = Shell {
        rt,
        wake,
        ep,
        main,
        fonts: scene::Fonts::default(),
    };

    // The first sink is WirePlumber's default: the quick slider shows it.
    create_node("sinka", "Audio/Sink");
    sh.until("sinka as the default sink", |rt, m| {
        rt.get(m, "sinkName").to_js_string() == "sinka" && rt.get(m, "sinkReady").truthy()
    });
    let sliders = sh.item("sliders");
    let quick = sh.slider(sliders, |w, i| {
        w.read(i, "materialSymbol")
            .map(|v| v.to_js_string())
            .as_deref()
            == Some("volume_up")
    });

    let v = sh.drag(quick, 0.4);
    assert!(v > 0.2 && v < 0.6, "dragged to {v}");
    let v = v as f32;
    server("sinka", "the quick slider's write", |n| {
        close(&n.volumes, &[v, v])
    });

    // Unbalanced channels (another client's [100%, 50%]): the slider sets their average and
    // keeps their ratio (PwNodeBoundAudio::setAverageVolume); dragged to its end, the louder
    // channel goes above 100%.
    let a = server("sinka", "sinka", |_| true);
    let id = a.id.to_string();
    pw_cli(&[
        "set-param",
        &id,
        "Props",
        "{ channelVolumes = [ 1.0, 0.125 ] }",
    ]);
    server("sinka", "the unbalanced volume", |n| {
        close(&n.volumes, &[1.0, 0.5])
    });
    // The slider's `Behavior on value` animates towards it; what it writes is where it is
    // dragged to, so the test waits for the shown average only.
    sh.until("the unbalanced volume shown", |rt, m| {
        (rt.get(m, "sinkVolume").to_number() - 0.75).abs() < 1e-3
    });
    let v = sh.drag(quick, 0.5) as f32;
    let half = [v * 4.0 / 3.0, v * 2.0 / 3.0];
    server("sinka", "the half-scale write", |n| {
        close(&n.volumes, &half)
    });
    let end = sh.drag(quick, 1.2);
    assert_eq!(end, 1.0);
    server("sinka", "the full-scale write", |n| {
        close(&n.volumes, &[4.0 / 3.0, 2.0 / 3.0])
    });

    // Another client above 100% (`wpctl set-volume @DEFAULT_SINK@ 150%`): shown, left alone.
    wpctl_set_volume(&id, "1.5");
    // The shell sees wpctl's 150% only when it dispatches the snapshot, so it runs meanwhile.
    sh.until("wpctl's 150% shown", |rt, m| {
        (rt.get(m, "sinkVolume").to_number() - 1.5).abs() < 1e-3
    });
    assert_eq!(wpctl_get_volume(&id), "Volume: 1.50");

    // The shell's own write above 100% reaches the server: wpctl, another client, reads it.
    sh.set_sink_volume(1.2);
    server("sinka", "the shell's 120%", |n| {
        close(&n.volumes, &[1.2, 1.2])
    });
    assert_eq!(wpctl_get_volume(&id), "Volume: 1.20");

    // The original's protection with "Volume limit" at 150%: a volume below it stays, one above it is
    // brought back to the last one below. Steps of 8% stay under "Max allowed increase".
    sh.call("protect", vec![Value::Bool(true), Value::Num(150.0)]);
    for (set, shown) in [("1.28", 1.28), ("1.36", 1.36), ("1.44", 1.44)] {
        wpctl_set_volume(&id, set);
        sh.until(&format!("wpctl's {set} shown"), |rt, m| {
            (rt.get(m, "sinkVolume").to_number() - shown).abs() < 1e-3
        });
    }
    assert_eq!(wpctl_get_volume(&id), "Volume: 1.44");
    assert_eq!(sh.rt.get(sh.main, "protections").to_number(), 0.0);
    wpctl_set_volume(&id, "1.52");
    sh.until(
        "the protection against the volume above the limit",
        |rt, m| rt.get(m, "protections").to_number() == 1.0,
    );
    // The protection's write goes out when the shell runs again, so it runs meanwhile. The
    // shell has seen the 152%, so a snapshot with 144% is a later one.
    sh.until("the volume brought back under the limit", |_, _| {
        qml::pw::snapshot().is_some_and(|s| {
            s.nodes
                .iter()
                .any(|n| n.name == "sinka" && close(&n.volumes, &[1.44, 1.44]))
        })
    });
    assert_eq!(wpctl_get_volume(&id), "Volume: 1.44");
    sh.call("protect", vec![Value::Bool(false), Value::Num(99.0)]);

    // An app stream in the mixer: its slider writes its own volume.
    create_node("mixstream", "Stream/Output/Audio");
    sh.until("the mixer entry's stream", |rt, m| {
        rt.get(m, "appReady").truthy()
    });
    let entry = sh.item("entry");
    let mixer = sh.slider(entry, |_, _| true);
    let v = sh.drag(mixer, 0.3) as f32;
    assert!(v > 0.1 && v < 0.5, "dragged to {v}");
    server("mixstream", "the mixer entry's write", |n| {
        close(&n.volumes, &[v, v])
    });

    // WirePlumber switches the default sink: the quick slider writes to the new one.
    create_node("sinkb", "Audio/Sink");
    let b = server("sinkb", "sinkb", |n| n.ready);
    let st = Command::new("wpctl")
        .args(["set-default", &b.id.to_string()])
        .status()
        .unwrap();
    assert!(st.success());
    sh.until("sinkb as the default sink", |rt, m| {
        rt.get(m, "sinkName").to_js_string() == "sinkb" && rt.get(m, "sinkReady").truthy()
    });
    let before = server("sinka", "sinka", |_| true).volumes;
    let v = sh.drag(quick, 0.2) as f32;
    server("sinkb", "the quick slider's write to sinkb", |n| {
        close(&n.volumes, &[v, v])
    });
    assert_eq!(server("sinka", "sinka", |_| true).volumes, before);

    // An ALSA-like sink whose volume lives in its card's Route: the quick slider sets the
    // Route; the Route's own change above 100% is shown, and a write above 100% goes to it.
    let card = RouteDevice::start("routecard", &[0.8, 0.8]);
    create_sink("routesink", card.id());
    let r = server("routesink", "routesink with its Route's volume", |n| {
        n.ready && close(&n.volumes, &[0.8, 0.8])
    });
    let st = Command::new("wpctl")
        .args(["set-default", &r.id.to_string()])
        .status()
        .unwrap();
    assert!(st.success());
    sh.until("routesink as the default sink", |rt, m| {
        rt.get(m, "sinkName").to_js_string() == "routesink"
            && (rt.get(m, "sinkVolume").to_number() - 0.8).abs() < 1e-3
    });
    let v = sh.drag(quick, 0.6) as f32;
    card.wait_set("the quick slider's write to the Route", |s| {
        close(&s.visual_volumes, &[v, v])
    });
    card.set_external(&[1.5, 0.75]);
    sh.until("the Route's 150% and 75% shown", |rt, m| {
        (rt.get(m, "sinkVolume").to_number() - 1.125).abs() < 1e-3
    });
    sh.set_sink_volume(1.5);
    card.wait_set("the write above 100% to the Route", |s| {
        close(&s.visual_volumes, &[2.0, 1.0])
    });

    drop(sh);
    drop(card);
    let _ = std::fs::remove_dir_all(&d);
    drop(kids);
}
