//! The editor draws the widget it holds once a refresh, so the pointer's motions are handled
//! once a refresh too: while a frame of the window is with the compositor, the motions read
//! are one step of the drag, delivered with that frame's callback. Each handled as it was
//! read (a 1000 Hz mouse) held the frame back by a hit test, the editor's frames and a
//! binding pass a motion. What QML itself drags is not held so: wayland_input.rs,
//! `a_burst_of_motions_drags_as_in_qt`.
//!
//! A frame callback delivers the pointer events read with it before it draws: a click among
//! them may close the very window the callback is for.

use std::process::{Child, Command, Stdio};

use wayland_client::protocol::{wl_output, wl_pointer, wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, delegate_noop};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};

const MANIFEST: &str = r#"
[[widget]]
type = "clock"
name = "Clock"
icon = "schedule"
placements = ["desktop"]
component = "W.qml"
size = { min = [40, 20], max = [800, 400], default = [200, 100] }
"#;

/// The edit overlay over the desktop a new layout has, with its clock, which the editor moves
/// with the pressed pointer: every motion handled asks for a frame.
const SHELL: &str = r#"import QtQuick
import Rustshell
import Rustshell.Wayland
import Impulse
ShellRoot {
    PanelWindow {
        id: win
        anchors { top: true; left: true; right: true }
        implicitHeight: 400
        DesktopContainer {
            id: desk
            connector: win.screen?.name ?? ""
            anchors.fill: parent
            Repeater {
                model: desk.widgets
                WidgetHost {
                    required property var modelData
                    // Mid-drag it is where the pointer has it (`shown`), not where it rests.
                    readonly property var box: desk.shown[modelData.id] ?? modelData
                    layoutId: modelData.id
                    widgetType: modelData.type
                    x: box.x
                    y: box.y
                    width: box.width
                    height: box.height
                }
            }
        }
        EditController {
            id: controller
            connector: win.screen?.name ?? ""
        }
        MouseArea {
            id: area
            property int moves: 0
            // Where the desktop shows its clock.
            function clockX(): int {
                const c = desk.widgets.find(w => w.type === "clock");
                return Math.round((desk.shown[c.id] ?? c).x);
            }
            function clockY(): int {
                const c = desk.widgets.find(w => w.type === "clock");
                return Math.round((desk.shown[c.id] ?? c).y);
            }
            property int from: 0
            anchors.fill: parent
            hoverEnabled: true
            onContainsMouseChanged: {
                if (containsMouse && controller.start())
                    console.log("TXT widget " + clockX() + " " + clockY());
            }
            onPressed: m => {
                from = clockX();
                controller.press(m.x, m.y);
            }
            onPositionChanged: m => {
                if (pressed) {
                    moves++;
                    controller.move(m.x, m.y);
                    console.log("TXT move " + moves);
                } else {
                    console.log("TXT at " + Math.round(m.x) + " " + Math.round(m.y));
                }
            }
            onReleased: m => {
                // Before the release is told: the last motion has been, held or not.
                console.log("TXT moves " + moves + " by " + (clockX() - from) + " held " + controller.dragging);
                controller.release(m.x, m.y);
            }
        }
    }
}
"#;

#[derive(Default)]
struct G {
    seat: Option<wl_seat::WlSeat>,
    vptr: Option<ZwlrVirtualPointerManagerV1>,
    output: Option<wl_output::WlOutput>,
    mode: (u32, u32),
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
                "wl_output" if g.output.is_none() => g.output = Some(reg.bind(name, 1, qh, ())),
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_output::WlOutput, ()> for G {
    fn event(
        g: &mut Self,
        _: &wl_output::WlOutput,
        ev: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Mode {
            flags,
            width,
            height,
            ..
        } = ev
            && flags
                .into_result()
                .is_ok_and(|f| f.contains(wl_output::Mode::Current))
        {
            g.mode = (width as u32, height as u32);
        }
    }
}

delegate_noop!(G: ignore wl_seat::WlSeat);
delegate_noop!(G: ZwlrVirtualPointerManagerV1);
delegate_noop!(G: ZwlrVirtualPointerV1);

/// The shell's `TXT` lines.
fn texts(out: &str) -> Vec<&str> {
    out.lines()
        .filter_map(|l| l.split_once("TXT ").map(|(_, t)| t))
        .collect()
}

/// Blocks until the shell's `TXT` lines satisfy `f`; panics if it exits first.
fn wait(log: &testenv::Log, what: &str, f: impl Fn(&[&str]) -> bool) {
    if let Err(out) = log.wait(|out| f(&texts(out))) {
        panic!(
            "the shell exited before {what}; it printed {:?}",
            texts(&out)
        );
    }
}

struct Shell(Child);

impl Drop for Shell {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A virtual pointer on the first output, at (50, 50); `None` where the compositor has none.
fn pointer() -> Option<(
    Connection,
    wayland_client::EventQueue<G>,
    G,
    ZwlrVirtualPointerV1,
)> {
    let Ok(conn) = Connection::connect_to_env() else {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return None;
    };
    let mut q = conn.new_event_queue();
    let qh = q.handle();
    conn.display().get_registry(&qh, ());
    let mut g = G::default();
    q.roundtrip(&mut g).unwrap();
    q.roundtrip(&mut g).unwrap();
    let (Some(seat), Some(vptr)) = (g.seat.clone(), g.vptr.clone()) else {
        testenv::unavailable("compositor lacks zwlr_virtual_pointer_v1");
        return None;
    };
    let ptr = match &g.output {
        Some(o) if vptr.version() >= 2 => {
            vptr.create_virtual_pointer_with_output(Some(&seat), Some(o), &qh, ())
        }
        _ => vptr.create_virtual_pointer(Some(&seat), &qh, ()),
    };
    let (w, h) = g.mode;
    assert!(w >= 700 && h >= 400, "first output mode {:?}", g.mode);
    ptr.motion_absolute(0, 50, 50, w, h);
    ptr.frame();
    q.roundtrip(&mut g).unwrap();
    Some((conn, q, g, ptr))
}

/// The shell on the `shell.qml` of `dir`, and what it prints.
fn shell(dir: &std::path::Path) -> (Shell, std::sync::Arc<testenv::Log>) {
    let xdg = dir.join("xdg");
    let (r, out) = std::io::pipe().unwrap();
    let sh = Shell(
        Command::new(env!("CARGO_BIN_EXE_impulse-shell"))
            .arg("-p")
            .arg(dir)
            .env("RI_INTERP", "1")
            .env("RI_LOG", "ri::frames=debug")
            .env("XDG_CONFIG_HOME", xdg.join("config"))
            .env("XDG_STATE_HOME", xdg.join("state"))
            .env("XDG_CACHE_HOME", xdg.join("cache"))
            .stdout(out.try_clone().unwrap())
            .stderr(out)
            .stdin(Stdio::null())
            .spawn()
            .unwrap(),
    );
    (sh, testenv::Log::capture(r))
}

#[test]
fn a_drag_handles_one_motion_a_frame_and_ends_where_the_pointer_is() {
    let Some((_conn, mut q, mut g, ptr)) = pointer() else {
        return;
    };
    let (w, h) = g.mode;

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("shell.qml"), SHELL).unwrap();
    std::fs::write(dir.path().join("widgets.toml"), MANIFEST).unwrap();
    std::fs::write(
        dir.path().join("W.qml"),
        "import QtQuick\nRectangle { color: \"red\" }\n",
    )
    .unwrap();
    let (sh, log) = shell(dir.path());
    // Where the desktop has its clock, once the editor is on.
    wait(&log, "the widget", |t| {
        t.iter().any(|l| l.starts_with("widget "))
    });
    let out = log.text();
    let at: Vec<u32> = texts(&out)
        .iter()
        .find_map(|l| l.strip_prefix("widget "))
        .unwrap()
        .split(' ')
        .filter_map(|p| p.parse().ok())
        .collect();
    let [x, y] = at[..] else {
        panic!("the shell printed {:?}", texts(&out));
    };
    let (x, y) = (x + 20, y + 50);
    ptr.motion_absolute(0, x, y, w, h);
    ptr.frame();
    q.roundtrip(&mut g).unwrap();
    // The shell has the pointer on the clock.
    wait(&log, "the pointer on the clock", |t| {
        t.iter().filter_map(|l| l.strip_prefix("at ")).any(|l| {
            let at: Vec<u32> = l.split(' ').filter_map(|p| p.parse().ok()).collect();
            matches!(at[..], [ax, ay] if ax.abs_diff(x) <= 1 && ay.abs_diff(y) <= 1)
        })
    });

    ptr.button(1, 0x110, wl_pointer::ButtonState::Pressed);
    ptr.frame();
    q.roundtrip(&mut g).unwrap();
    // The drag begins, and the frames of it come: the callback of the first one is here.
    const BEGIN: u32 = 10;
    for i in 1..=BEGIN {
        ptr.motion_absolute(1 + i, x + i, y + i / 2, w, h);
        ptr.frame();
        q.roundtrip(&mut g).unwrap();
    }
    let before = log.text().len();
    log.wait(|out| out[before..].contains("cb win"))
        .unwrap_or_else(|e| panic!("no frame callback in the drag: {e}"));
    // A fast mouse: many motions between two frames of the screen, each in its own
    // wl_pointer.frame and each with the shell before the next is sent; every tenth waits
    // for the shell to have handled one more. Down as well as across: the desktop keeps the
    // clock off the widget beside it.
    const MOTIONS: u32 = 290;
    let handled = |t: &[&str]| t.iter().filter(|l| l.starts_with("move ")).count() as u32;
    let before = log.text().len();
    let began = handled(&texts(&log.text()));
    for i in BEGIN + 1..=BEGIN + MOTIONS {
        let had = handled(&texts(&log.text()));
        ptr.motion_absolute(1 + i, x + i, y + i / 2, w, h);
        ptr.frame();
        q.roundtrip(&mut g).unwrap();
        if i % 10 == 0 {
            wait(&log, "a motion of the drag", |t| handled(t) > had);
        }
    }
    ptr.button(
        2 + BEGIN + MOTIONS,
        0x110,
        wl_pointer::ButtonState::Released,
    );
    ptr.frame();
    q.roundtrip(&mut g).unwrap();
    wait(&log, "the release", |t| {
        t.iter().any(|l| l.starts_with("moves "))
    });
    let out = log.text();
    let line = texts(&out)
        .iter()
        .find(|l| l.starts_with("moves "))
        .unwrap()
        .to_string();
    assert!(
        line.ends_with("held true"),
        "the editor held no widget: {line}"
    );
    let n: Vec<u32> = line.split(' ').filter_map(|p| p.parse().ok()).collect();
    let [moves, by] = n[..] else {
        panic!("the shell printed {line:?}");
    };
    // The release has the widget at the last motion, held or not: as far as the pointer
    // went, less the 5 px it goes before it is a drag.
    let all = BEGIN + MOTIONS;
    assert!((all - 6..=all).contains(&by), "{line}");
    let moves = moves - began;
    // The frames of the drag: the frame callbacks of the window that had the most.
    let mut frames = std::collections::HashMap::<&str, u32>::new();
    for l in out[before..].lines() {
        if let Some((_, cb)) = l.split_once("cb win") {
            *frames.entry(cb.split(' ').next().unwrap()).or_default() += 1;
        }
    }
    let frames = frames.into_values().max().unwrap_or(0);
    // A refresh has its first motion as it is read and the last of those held, ahead of the
    // callback: two a frame, and the few before the first frame. Each handled as it was read,
    // nearly all of them.
    assert!(
        moves >= MOTIONS / 10 && moves <= 2 * frames + 6,
        "{moves} of {MOTIONS} motions handled in {frames} frames"
    );
    drop(sh);
}

/// A window that draws every refresh and closes at a click, shown again 60 ms later. Its
/// press handler takes 40 ms: the release, sent once the handler says it has begun, and the
/// callback of the frame the window had with the compositor both come meanwhile, and are
/// read at once.
const CLOSING: &str = r#"import QtQuick
import Rustshell
import Rustshell.Wayland
ShellRoot {
    PanelWindow {
        id: win
        property int closed: 0
        anchors { top: true; left: true; right: true }
        implicitHeight: 400
        Rectangle {
            width: 40; height: 40; color: "red"
            NumberAnimation on x { from: 0; to: 300; duration: 500; loops: Animation.Infinite }
        }
        MouseArea {
            anchors.fill: parent
            hoverEnabled: true
            onContainsMouseChanged: {
                if (containsMouse)
                    console.log("TXT over " + win.closed);
            }
            onPressed: {
                console.log("TXT pressed " + win.closed);
                const t = Date.now();
                while (Date.now() - t < 40) {}
            }
            onClicked: {
                win.closed++;
                win.visible = false;
                again.start();
                console.log("TXT closed " + win.closed);
            }
        }
    }
    Timer {
        id: again
        interval: 60
        onTriggered: {
            win.visible = true;
            console.log("TXT shown " + win.closed);
        }
    }
}
"#;

#[test]
fn a_click_read_with_a_frame_callback_may_close_the_callbacks_window() {
    let Some((conn, mut q, mut g, ptr)) = pointer() else {
        return;
    };
    let (w, h) = g.mode;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("shell.qml"), CLOSING).unwrap();
    let (sh, log) = shell(dir.path());
    let has = |line: String| move |t: &[&str]| t.contains(&line.as_str());
    const CLICKS: u32 = 30;
    let mut time = 1;
    for n in 0..=CLICKS {
        if n > 0 {
            wait(
                &log,
                &format!("the window shown after click {n}"),
                has(format!("shown {n}")),
            );
        }
        // The window shown again has the pointer once it moves.
        for dx in [0, 1] {
            ptr.motion_absolute(time, 100 + dx, 100, w, h);
            ptr.frame();
            time += 1;
        }
        q.roundtrip(&mut g).unwrap();
        // After the last click too: the shell that answers the pointer is running.
        wait(
            &log,
            &format!("the pointer in the window shown after click {n}"),
            has(format!("over {n}")),
        );
        if n == CLICKS {
            break;
        }
        ptr.button(time, 0x110, wl_pointer::ButtonState::Pressed);
        ptr.frame();
        conn.flush().unwrap();
        wait(
            &log,
            &format!("press {}", n + 1),
            has(format!("pressed {n}")),
        );
        ptr.button(time + 1, 0x110, wl_pointer::ButtonState::Released);
        ptr.frame();
        conn.flush().unwrap();
        time += 2;
        wait(
            &log,
            &format!("click {} to close the window", n + 1),
            has(format!("closed {}", n + 1)),
        );
    }
    drop(sh);
}
