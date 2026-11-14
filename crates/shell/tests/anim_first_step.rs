//! An animation a press handler starts takes its first step after the frame that shows the
//! press, never in the pass that handled the press. In Qt Quick the animation driver starts
//! from the event loop after the handler returned, and the render loop advances it after a
//! sync (QSGThreadedRenderLoop::polishAndSync), so the frame that answers the press shows the
//! animation at its start. Quickshell on Qt 6.12 under a headless sway, ten presses of an idle
//! window, each handler changing something on screen: one or two syncs show the start value
//! (the press's frame, and at times the started driver's, which has nothing new to draw), then
//! comes the first step: about a frame after the press, or 1 to 3 ms after it when the render
//! loop did not wait for sway; the second step follows the next sync.
//!
//! What a frame shows is read where each shell decides it: Qt's QQuickWindow::afterAnimating
//! (the GUI thread, before each sync; its render thread commits on its own time, so the order
//! of its commits and its steps in one log says nothing), and rusty-impulse's wl_surface.commit (one
//! thread draws and steps). Each shell runs on a sway of its own.

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use wayland_client::protocol::{wl_pointer::ButtonState, wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, QueueHandle, delegate_noop};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};

const PRESSES: usize = 10;

const SCREEN: (u32, u32) = (1920, 1080);

/// `FRAMES` is Qt's frame marker, or nothing. `unseen` takes `area.v` and `area.presses` out
/// of what the panel draws.
const SHELL: &str = r##"import QtQuick
import Quickshell
ShellRoot {
    PanelWindow {
        anchors { top: true; left: true }
        implicitWidth: 200; implicitHeight: 100
        exclusionMode: ExclusionMode.Ignore
        color: "#202020"
        MouseArea {
            id: area
            anchors.fill: parent
            hoverEnabled: true
            property real v: 0
            property int presses: 0
            onContainsMouseChanged: console.log("TXT hover " + containsMouse)
            onVChanged: if (v > 0) console.log("TXT step " + Date.now())
            onPressed: {
                console.log("TXT press " + Date.now());
                presses += 1;
                grow.restart();
            }
            Rectangle { width: 4 + area.v; height: 20; color: "#ff0000" }
            Rectangle { y: 40; width: 4 + 8 * area.presses; height: 20; color: "#00ff00" }
            NumberAnimation {
                id: grow
                target: area; property: "v"; from: 0; to: 100; duration: 300
                onFinished: console.log("TXT finished")
            }
            FRAMES
        }
    }
}
"##;

const QT_FRAMES: &str = r#"Connections {
                target: area.Window.window
                function onAfterAnimating() { console.log("TXT frame"); }
            }"#;

#[derive(Default)]
struct G {
    seat: Option<wl_seat::WlSeat>,
    vptr: Option<ZwlrVirtualPointerManagerV1>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for G {
    fn event(
        g: &mut Self,
        reg: &wl_registry::WlRegistry,
        ev: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = ev
        {
            match interface.as_str() {
                "wl_seat" if g.seat.is_none() => g.seat = Some(reg.bind(name, 1, qh, ())),
                "zwlr_virtual_pointer_manager_v1" => {
                    g.vptr = Some(reg.bind(name, version.min(2), qh, ()))
                }
                _ => {}
            }
        }
    }
}

delegate_noop!(G: ignore wl_seat::WlSeat);
delegate_noop!(G: ZwlrVirtualPointerManagerV1);
delegate_noop!(G: ZwlrVirtualPointerV1);

struct Shell(Child);

impl Drop for Shell {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// What one press did until its animation's third step.
#[derive(Debug)]
struct Press {
    /// Frames between the press and the animation's first step.
    frames_before_first_step: usize,
    /// When the first and the second step ran, ms after the press.
    steps: [i64; 2],
}

/// The number after `TXT <what> `.
fn stamp(line: &str, what: &str) -> Option<i64> {
    let (_, r) = line.split_once(&format!("TXT {what} "))?;
    r.trim().parse().ok()
}

fn presses(log: &str, qt: bool) -> Vec<Press> {
    let lines: Vec<&str> = log.lines().collect();
    let frame = |l: &str| match qt {
        true => l.contains("TXT frame"),
        false => l.contains("-> wl_surface#") && l.contains(".commit()"),
    };
    (lines.iter().enumerate())
        .filter_map(|(i, l)| Some((i, stamp(l, "press")?)))
        .map(|(i, at)| {
            let after = &lines[i + 1..];
            let first = (after.iter().position(|l| stamp(l, "step").is_some()))
                .expect("a step after each press");
            let mut steps = after.iter().filter_map(|l| stamp(l, "step"));
            Press {
                frames_before_first_step: after[..first].iter().filter(|l| frame(l)).count(),
                steps: [(); 2].map(|()| steps.next().expect("two steps after each press") - at),
            }
        })
        .collect()
}

/// The shell has nothing left to do for its window: sway answered every frame callback it
/// asked for, and after its last commit it went to wait for events. rusty-impulse says so in its loop's
/// turns (`RI_LOG=ri::sched`: a turn with nothing due, after whatever the scene or sway last
/// said), Qt in its render loop's log (the render thread waits, and the window asked for no
/// update since: that is a line of the same log).
fn idle(log: &str, qt: bool) -> bool {
    let answered = (log.split(".frame(new id wl_callback#").skip(1)).all(|r| {
        let id = r.split(')').next().unwrap_or_default();
        r.contains(&format!("wl_callback#{id}.done("))
    });
    let lines: Vec<&str> = log.lines().collect();
    let last = |f: &dyn Fn(&str) -> bool| lines.iter().rposition(|l| f(l));
    let (said, waits) = match qt {
        true => (
            last(&|l| l.contains(".commit()")),
            last(&|l| {
                l.contains("qt.scenegraph.renderloop")
                    && !QT_SYNC_ENDS.iter().any(|e| l.contains(e))
            })
            .filter(|&i| lines[i].contains(QT_WAITS)),
        ),
        false => (
            last(&|l| l.contains("TXT ") || l.contains(".done(") || l.contains(".commit()")),
            last(&|l| testenv::wayland::turn_time(l).is_some())
                .filter(|&i| lines[i].contains(" next None")),
        ),
    };
    answered && waits.is_some_and(|w| said.is_none_or(|s| s < w))
}

/// The render thread's line when it has drawn and waits for the window's next update.
const QT_WAITS: &str = "begin processEventsAndWaitForMore()";

/// What the GUI thread writes when a sync is over. The render thread may be waiting by then,
/// after an expose it always is close to: these lines ask for nothing.
const QT_SYNC_ENDS: [&str; 2] = ["- unlock after sync", "- done with handleExposure()"];

/// Runs `qs` or rusty-impulse on a sway of its own and presses its panel `PRESSES` times, each once
/// the previous animation finished and its window is idle. `seen`: the press and the
/// animation change what the panel draws.
fn run(qs: Option<&Path>, seen: bool) -> Vec<Press> {
    let sway = testenv::Sway::start();
    let conn = Connection::from_socket(UnixStream::connect(sway.display()).unwrap()).unwrap();
    let mut q = conn.new_event_queue();
    let qh = q.handle();
    conn.display().get_registry(&qh, ());
    let mut g = G::default();
    q.roundtrip(&mut g).unwrap();
    let (seat, vptr) = (g.seat.clone().unwrap(), g.vptr.clone().unwrap());
    let ptr = vptr.create_virtual_pointer(Some(&seat), &qh, ());

    let dir = tempfile::tempdir().unwrap();
    let frames = if qs.is_some() { QT_FRAMES } else { "" };
    let mut shell = SHELL.replace("FRAMES", frames);
    if !seen {
        shell = shell
            .replace("4 + area.v", "4")
            .replace("4 + 8 * area.presses", "4");
    }
    std::fs::write(dir.path().join("shell.qml"), shell).unwrap();
    let (r, out) = std::io::pipe().unwrap();
    // libwayland writes its log to stderr in pieces: line-buffered, a line of it stays whole.
    let mut cmd = Command::new("stdbuf");
    cmd.arg("-eL")
        .arg(qs.unwrap_or(Path::new(env!("CARGO_BIN_EXE_rusty-impulse"))));
    sway.env(&mut cmd)
        .arg("-p")
        .arg(dir.path())
        .envs(testenv::home_vars(&dir.path().join("home")))
        .env("RI_INTERP", "1")
        .env("QT_QPA_PLATFORM", "wayland")
        .env("WAYLAND_DEBUG", "1")
        .env("QT_LOGGING_RULES", "qt.scenegraph.renderloop=true")
        .env("RI_LOG", "ri::sched")
        .stdout(out.try_clone().unwrap())
        .stderr(out)
        .stdin(Stdio::null());
    let _sh = Shell(cmd.spawn().unwrap());
    // Its copies of the pipe's write end: the log ends when the shell exits.
    drop(cmd);
    let log = testenv::Log::capture(r);
    let wait = |what: &str, f: &dyn Fn(&str) -> bool| {
        if let Err(out) = log.wait(f) {
            let tail: Vec<&str> = out.lines().rev().take(30).collect();
            panic!("the shell (qs: {qs:?}) exited before {what}:\n{tail:#?}");
        }
    };
    // Sway mapped the panel: its surface entered the output.
    wait("its panel is on screen", &|s| {
        (s.split_once("zwlr_layer_surface_v1#"))
            .is_some_and(|(_, r)| r.contains(".enter(wl_output#"))
    });
    ptr.motion_absolute(0, 50, 50, SCREEN.0, SCREEN.1);
    ptr.frame();
    q.roundtrip(&mut g).unwrap();
    wait("the pointer entered its panel", &|s| {
        s.contains("TXT hover true")
    });
    for n in 0..PRESSES {
        wait("the window is idle", &|s| {
            s.matches("TXT finished").count() >= n && idle(s, qs.is_some())
        });
        for (t, state) in [(0, ButtonState::Pressed), (1, ButtonState::Released)] {
            ptr.button(2 * n as u32 + t, 0x110, state);
            ptr.frame();
            q.roundtrip(&mut g).unwrap();
        }
    }
    wait("the last animation finished", &|s| {
        s.matches("TXT finished").count() >= PRESSES
    });
    presses(&log.text(), qs.is_some())
}

fn table(name: &str, p: &[Press]) -> String {
    let rows: String = (p.iter().enumerate())
        .map(|(i, p)| {
            format!(
                "  press {:2}: {} frame(s) before the first step; steps at {} and {} ms\n",
                i + 1,
                p.frames_before_first_step,
                p.steps[0],
                p.steps[1]
            )
        })
        .collect();
    format!("{name}:\n{rows}")
}

fn qs() -> Option<PathBuf> {
    (std::env::var_os("RI_IACT_QS").map(PathBuf::from)).or_else(|| {
        std::env::split_paths(&std::env::var_os("PATH")?)
            .map(|d| d.join("qs"))
            .find(|p| p.is_file())
    })
}

#[test]
fn fails_without_sway() {
    testenv::assert_fails_when_missing(
        "first_step_follows_the_frame_that_shows_the_press",
        &[("PATH", Some("/nonexistent"))],
    );
}

#[test]
fn first_step_follows_the_frame_that_shows_the_press() {
    if !testenv::require("sway") {
        return;
    }
    let ours = run(None, true);
    let mut report = table("rusty-impulse", &ours);
    if let Some(qs) = qs() {
        let qt = run(Some(&qs), true);
        report += &table("qs (Qt)", &qt);
        assert!(
            qt.len() == PRESSES && qt.iter().all(|p| p.frames_before_first_step >= 1),
            "Qt no longer shows the press before the first step:\n{report}"
        );
    }
    eprintln!("{report}");
    assert_eq!(ours.len(), PRESSES, "{report}");
    assert!(
        ours.iter().all(|p| p.frames_before_first_step >= 1),
        "a first step ran before the frame that shows the press:\n{report}"
    );
}

/// Neither the press nor the animation changes a pixel. Quickshell on Qt 6.12 syncs a frame
/// all the same and takes the first step 0 to 2 ms after the press. rusty-impulse draws no frame
/// that changes nothing, so there is none to wait for: its first step comes by the vsync
/// timer, 16 to 17 ms after the press. Both step; rusty-impulse once waited for a frame without end.
#[test]
fn an_animation_that_draws_nothing_steps_without_a_frame() {
    if !testenv::require("sway") {
        return;
    }
    let ours = run(None, false);
    let mut report = table("rusty-impulse", &ours);
    let late = |p: &[Press]| p.iter().map(|p| p.steps[0]).max().unwrap_or(0);
    if let Some(qs) = qs() {
        let qt = run(Some(&qs), false);
        report += &table("qs (Qt)", &qt);
        assert!(
            qt.len() == PRESSES && late(&qt) < 100,
            "Qt no longer steps such an animation within 100 ms of the press:\n{report}"
        );
    }
    eprintln!("{report}");
    assert_eq!(ours.len(), PRESSES, "{report}");
    assert!(
        late(&ours) < 100,
        "a first step came 100 ms or more after the press:\n{report}"
    );
}
