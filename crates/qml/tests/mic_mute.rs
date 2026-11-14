//! The bar's microphone button (`Audio.toggleMicMute()`, the `Audio` service of qml) against
//! the command it replaces, `wpctl set-mute @DEFAULT_SOURCE@ toggle`: the same clicks through
//! the real `wpctl` and through the service, on a private pipewire + wireplumber with a null
//! sink and a null source, must leave the default nodes in the same state after every step
//! (read back with `wpctl get-volume`). Fails when they are not installed (`testenv`).
//!
//! `wpctl` gets the nodes by id, not as `@DEFAULT_SOURCE@`: the test waits for the daemon to
//! report which nodes are the defaults and passes their ids. A `wpctl` started on a busy
//! machine can run its command before it has received the `default` metadata; `@DEFAULT_…@`
//! then names no node, `set-mute` does nothing (exit 3) and `get-volume` prints nothing (exit 0).
#![cfg(feature = "pipewire")]

use qml::runtime::Runtime;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;
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
/// the shell, its daemons or tools elsewhere. `PATH` holds
/// no `wpctl` from the start, so the shell cannot fall back to running it (libpipewire on the
/// runtime's pipewire thread reads the environment with getenv(3)); the tools are found on
/// `TOOLS_PATH`, the caller's PATH.
fn private_session(dir: &Path) -> Vec<(&'static str, OsString)> {
    std::fs::create_dir_all(dir.join("state")).unwrap();
    let mut bus = OsString::from("unix:path=");
    bus.push(dir.join("bus"));
    vec![
        ("XDG_RUNTIME_DIR", dir.into()),
        ("PIPEWIRE_RUNTIME_DIR", dir.into()),
        ("XDG_STATE_HOME", dir.join("state").into()),
        ("DBUS_SESSION_BUS_ADDRESS", bus),
        ("PATH", dir.join("empty").into()),
        (TOOLS_PATH, std::env::var_os("PATH").unwrap_or_default()),
    ]
}

const TOOLS_PATH: &str = "WPCTL_MUTE_TOOLS_PATH";

/// What the test's own process runs without.
const UNSET: &[&str] = &["PIPEWIRE_REMOTE"];

/// `cmd` as the session's own daemons and tools run: found on [`TOOLS_PATH`].
fn tool(cmd: &str) -> Command {
    let mut c = Command::new(cmd);
    c.env("PATH", std::env::var_os(TOOLS_PATH).unwrap());
    c
}

fn spawn(kids: &mut Kill, cmd: &str, args: &[&str]) -> Option<()> {
    spawn_cmd(kids, tool(cmd).args(args), cmd)
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

/// Runs the shell's events until none is pending.
fn drain(rt: &mut Runtime, wake: &Wakeups, ep: Option<i32>) {
    loop {
        rt.poll_native();
        rt.settle();
        let woke = wake.wait(ep, Some(Duration::ZERO));
        if woke.fd {
            rt.process_fd_ready();
        }
        if !woke.any() {
            return;
        }
    }
}

/// Whether the daemon reports node `name` with `muted`.
fn daemon_muted(s: &qml::pw::Snap, name: &str, muted: bool) -> bool {
    s.nodes.iter().any(|n| n.name == name && n.muted == muted)
}

const QML: &str = r#"import QtQuick
import ri.services
Item {
    readonly property bool ready: Audio.sink?.audio != null && Audio.source?.audio != null
    readonly property bool sinkMuted: Audio.sink?.audio?.muted ?? false
    readonly property bool sourceMuted: Audio.source?.audio?.muted ?? false
    function click() { Audio.toggleMicMute() }
}
"#;

/// The bar's button on a default source that has no `audio` (`UtilButtons.qml`).
const VIRTUAL_QML: &str = r#"import QtQuick
import Rustshell.Services.Pipewire
import ri.services
import Impulse
Item {
    readonly property bool ready: Audio.sink?.audio != null && Pipewire.defaultAudioSource !== null && Audio.source?.audio == null
    readonly property bool sinkMuted: Audio.sink?.audio?.muted ?? false
    property bool sourceMuted: false
    function click() {
        Audio.source?.audio ? Audio.toggleMicMute() : Microphone.toggleMute()
        sourceMuted = !sourceMuted
    }
}
"#;

/// How often the button is clicked.
const CLICKS: usize = 3;

fn ri() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

/// The button calls the service: UtilButtons.qml's microphone button and the quick toggle.
#[test]
fn mic_buttons_call_the_audio_service() {
    for f in [
        "modules/rusty/bar/UtilButtons.qml",
        "modules/common/models/quickToggles/MicToggle.qml",
    ] {
        let text = std::fs::read_to_string(ri().join(f)).unwrap();
        assert!(text.contains("Audio.toggleMicMute()"), "{f}");
        assert!(!text.contains("wpctl"), "{f}");
    }
    let bar = std::fs::read_to_string(ri().join("modules/rusty/bar/UtilButtons.qml")).unwrap();
    let click = "Audio.source?.audio ? Audio.toggleMicMute() : Microphone.toggleMute()";
    assert!(bar.contains(click) && VIRTUAL_QML.contains(click));
}

/// `wpctl get-volume` of the nodes `ids`.
fn state(ids: &[String; 2]) -> String {
    ids.each_ref()
        .map(|id| {
            let o = tool("wpctl").args(["get-volume", id]).output().unwrap();
            let out = String::from_utf8_lossy(&o.stdout).trim().to_string();
            // It exits 0 without a line when it did not find the node or its volume.
            assert!(
                o.status.success() && out.starts_with("Volume:"),
                "wpctl get-volume {id}: {:?} {out:?} {}",
                o.status,
                String::from_utf8_lossy(&o.stderr)
            );
            out
        })
        .join(" | ")
}

#[test]
fn mic_toggle_matches_wpctl() {
    const TEST: &str = concat!(module_path!(), "::mic_toggle_matches_wpctl");
    toggle_matches_wpctl(TEST, "Audio/Source", QML);
}

/// A default source of media.class `Audio/Source/Virtual` (a filter's, a null sink's): quickshell
/// gives it no `audio`, and the original's bar button mutes it all the same.
#[test]
fn virtual_mic_toggle_matches_wpctl() {
    const TEST: &str = concat!(module_path!(), "::virtual_mic_toggle_matches_wpctl");
    toggle_matches_wpctl(TEST, "Audio/Source/Virtual", VIRTUAL_QML);
}

/// `test` in its own process with a private session (`private_session`): `CLICKS` clicks of
/// `qml`'s `click()` on a default source of media.class `source_class`.
fn toggle_matches_wpctl(test: &str, source_class: &str, qml: &str) {
    if !testenv::in_own_process(test) {
        for t in ["dbus-daemon", "pipewire", "wireplumber", "pw-cli", "wpctl"] {
            if !testenv::require(t) {
                return;
            }
        }
        let dir = tempfile::tempdir().unwrap();
        testenv::run_in_own_process_env(test, private_session(dir.path()), UNSET);
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
        || spawn_cmd(
            &mut kids,
            socket_activated_pipewire(&dir).env("PATH", std::env::var_os(TOOLS_PATH).unwrap()),
            "pipewire",
        )
        .is_none()
        || spawn(&mut kids, "wireplumber", &[]).is_none()
    {
        return;
    }
    for class in ["Audio/Sink", source_class] {
        let name = if class == source_class {
            "regsrc"
        } else {
            "regsink"
        };
        let node = format!(
            "{{ factory.name=support.null-audio-sink media.class={class} node.name={name} audio.position=[FL,FR] object.linger=true }}"
        );
        let st = tool("pw-cli")
            .args(["create-node", "adapter", &node])
            .output()
            .unwrap();
        assert!(st.status.success(), "{st:?}");
    }
    // wireplumber made them the defaults, and the daemon reports their Props.
    let snap = qml::pw::wait_snapshot_until(|s| {
        let ready = |name: &str| s.nodes.iter().any(|n| n.name == name && n.ready);
        s.defaults[..2] == ["regsink", "regsrc"] && ready("regsink") && ready("regsrc")
    });
    // The default sink and source, as the daemon reported them.
    let ids = [0, 1].map(|d| {
        let node = snap.nodes.iter().find(|n| n.name == snap.defaults[d]);
        node.unwrap().id.to_string()
    });
    let state = || state(&ids);
    let reset = || {
        for id in &ids {
            assert!(
                tool("wpctl")
                    .args(["set-mute", id, "0"])
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let s = state();
        assert!(!s.contains("MUTED"), "{s}");
        s
    };

    let mut orig = vec![reset()];
    for _ in 0..CLICKS {
        let st = tool("wpctl")
            .args(["set-mute", &ids[1], "toggle"])
            .status()
            .unwrap();
        assert!(st.success());
        orig.push(state());
    }

    let mut native = vec![reset()];
    // No `wpctl` on this process's PATH (`private_session`): the shell cannot run it.
    let mut rt = Runtime::new(Some(ri()));
    rt.load_singletons_lazy(&[
        ri(),
        ri().join("services"),
        ri().join("modules/common"),
        ri().join("modules/common/functions"),
    ]);
    let wake = Wakeups::new();
    rt.world.borrow().waker.set_ping(wake.ping());
    let ep = rt.process_fd();
    // A file, not `load_str`: `import ri.services` resolves per document.
    let doc = dir.join("MicCase.qml");
    std::fs::write(&doc, qml).unwrap();
    let root = rt
        .load_file(&doc)
        .unwrap_or_else(|e| panic!("load: {e:?} {:?}", rt.errors()));
    run_until(&mut rt, &wake, ep, |rt| rt.get(root, "ready").truthy());
    for _ in 0..CLICKS {
        rt.call_method(root, "click", vec![]).unwrap().unwrap();
        // The write is out to the daemon; then the daemon reports what the shell shows.
        drain(&mut rt, &wake, ep);
        let sink = rt.get(root, "sinkMuted").truthy();
        let source = rt.get(root, "sourceMuted").truthy();
        qml::pw::wait_snapshot_until(|s| {
            s.defaults[..2] == ["regsink", "regsrc"]
                && daemon_muted(s, "regsink", sink)
                && daemon_muted(s, "regsrc", source)
        });
        native.push(state());
    }
    assert_eq!(native, orig);
    assert!(orig.iter().any(|s| s.contains("MUTED")), "{orig:?}");
}
