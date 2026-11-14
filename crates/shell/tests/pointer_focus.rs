//! Pointer focus belongs to a wl_surface (wl_pointer.enter/leave name one): when a bar is
//! replaced by a new surface for the same window while the pointer is over it, the old
//! surface retires and its hover ends, so the successor's enter is a fresh hover.

use std::process::{Child, Command, Stdio};

use wayland_client::protocol::{wl_output, wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, delegate_noop};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};

/// The hovered bar moves to the bottom edge under a new namespace, which layer-shell can only
/// give a new layer surface; the old one, reserving space, stays mapped until its successor maps.
const SHELL: &str = r#"import QtQuick
import Rustshell
import Rustshell.Wayland
ShellRoot {
    id: root
    property bool moved: false
    PanelWindow {
        WlrLayershell.namespace: root.moved ? "ptrfocus:bottom" : "ptrfocus:top"
        anchors { top: !root.moved; bottom: root.moved; left: true; right: true }
        exclusiveZone: 100
        implicitHeight: 100
        MouseArea {
            anchors.fill: parent
            hoverEnabled: true
            onContainsMouseChanged: {
                console.log("TXT hover " + containsMouse);
                if (containsMouse) root.moved = true;
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

/// Before the fix the old surface's wl_pointer.leave found no window and was dropped, so the
/// MouseArea stayed hovered through the move and the successor's enter changed nothing.
#[test]
fn replaced_bar_surface_ends_hover() {
    let Ok(conn) = Connection::connect_to_env() else {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return;
    };
    let mut q = conn.new_event_queue();
    let qh = q.handle();
    conn.display().get_registry(&qh, ());
    let mut g = G::default();
    q.roundtrip(&mut g).unwrap();
    q.roundtrip(&mut g).unwrap();
    let (Some(seat), Some(vptr)) = (g.seat.clone(), g.vptr.clone()) else {
        testenv::unavailable("compositor lacks zwlr_virtual_pointer_v1");
        return;
    };
    let ptr = match &g.output {
        Some(o) if vptr.version() >= 2 => {
            vptr.create_virtual_pointer_with_output(Some(&seat), Some(o), &qh, ())
        }
        _ => vptr.create_virtual_pointer(Some(&seat), &qh, ()),
    };
    let (w, h) = g.mode;
    assert!(w >= 100 && h >= 300, "first output mode {:?}", g.mode);
    // The pointer waits where the bar maps: sway rebases the cursor when a layer surface maps.
    ptr.motion_absolute(0, 50, 50, w, h);
    ptr.frame();
    q.roundtrip(&mut g).unwrap();

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("shell.qml"), SHELL).unwrap();
    let xdg = dir.path().join("xdg");
    let (r, out) = std::io::pipe().unwrap();
    let sh = Shell(
        Command::new(env!("CARGO_BIN_EXE_rusty-impulse"))
            .arg("-p")
            .arg(dir.path())
            .env("RI_INTERP", "1")
            .env("XDG_CONFIG_HOME", xdg.join("config"))
            .env("XDG_STATE_HOME", xdg.join("state"))
            .env("XDG_CACHE_HOME", xdg.join("cache"))
            .stdout(out.try_clone().unwrap())
            .stderr(out)
            .stdin(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let log = testenv::Log::capture(r);

    wait(&log, "the top bar's hover", |t| {
        t.starts_with(&["hover true"])
    });
    wait(&log, "the hover to end with the top bar's surface", |t| {
        t.starts_with(&["hover true", "hover false"])
    });
    ptr.motion_absolute(1, 50, h - 50, w, h);
    ptr.frame();
    q.roundtrip(&mut g).unwrap();
    wait(&log, "the bottom bar's hover", |t| {
        t.starts_with(&["hover true", "hover false", "hover true"])
    });
    drop(sh);
}
