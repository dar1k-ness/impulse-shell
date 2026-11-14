//! An animation that changes no pixel is stepped by the vsync timer, 60 times a second on a
//! 60 Hz output, whatever else wakes the loop: QSGThreadedRenderLoop's animation timer fires
//! once its thread is back in the event loop, however busy the thread was when it timed out.
//! The real quickshell running the fixture vs the native shell (`rusty-impulse -p`): the steps of a
//! NumberAnimation of a property nothing draws, in the second a Timer counts: alone, while
//! the pointer moves over the window as often as the compositor takes a move, and next to a
//! function that posts itself again with Qt.callLater.
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use testenv::Log;
use wayland_client::protocol::{wl_output, wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, delegate_noop};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};

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

/// A virtual pointer that crosses the first output again and again, a move each time the
/// compositor has answered the one before, until dropped.
struct Moving {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Moving {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Round trips to the compositor between two moves: a move per round trip is tens of
/// thousands a second, more than a window under the pointer keeps its frames with.
const PER_MOVE: u32 = 16;

/// None where the compositor has no zwlr_virtual_pointer_v1.
fn moving() -> Option<Moving> {
    let conn = Connection::connect_to_env().ok()?;
    let mut q = conn.new_event_queue();
    let qh = q.handle();
    conn.display().get_registry(&qh, ());
    let mut g = G::default();
    q.roundtrip(&mut g).unwrap();
    q.roundtrip(&mut g).unwrap();
    let (seat, vptr) = (g.seat.clone()?, g.vptr.clone()?);
    let ptr = match &g.output {
        Some(o) if vptr.version() >= 2 => {
            vptr.create_virtual_pointer_with_output(Some(&seat), Some(o), &qh, ())
        }
        _ => vptr.create_virtual_pointer(Some(&seat), &qh, ()),
    };
    let (w, h) = g.mode;
    assert!(w >= 100 && h >= 100, "first output mode {:?}", g.mode);
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let thread = std::thread::spawn(move || {
        let mut k = 0u32;
        while !stopped.load(Ordering::Relaxed) {
            ptr.motion_absolute(k, 20 + k % (w - 40), h / 2, w, h);
            ptr.frame();
            // The compositor's answer paces the moves.
            for _ in 0..PER_MOVE {
                if q.roundtrip(&mut g).is_err() {
                    return;
                }
            }
            k += 1;
        }
    });
    Some(Moving {
        stop,
        thread: Some(thread),
    })
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Half a second for the window to appear, then the animation starts and the next firing of
/// the Timer, one second later, reports. `BUSY`: the Qt.callLater chain starts with the
/// animation; `moves` counts its calls too.
const SHELL: &str = r#"import QtQuick
import Quickshell

FloatingWindow {
    id: root
    implicitWidth: 200
    implicitHeight: 200
    property int moves: 0
    property int steps: 0
    property real v: 0
    onVChanged: root.steps++
    property real t0: 0
    Rectangle { width: 10; height: 10; color: "red" }
    NumberAnimation { id: stepper; target: root; property: "v"; from: 0; to: 1; duration: 3600000 }
    MouseArea { anchors.fill: parent; hoverEnabled: true; onPositionChanged: root.moves++ }
    function spin() { root.moves++; Qt.callLater(root.spin) }
    Timer {
        interval: 500; running: true; repeat: true
        onTriggered: {
            if (root.t0 == 0) {
                interval = 1000
                root.t0 = Date.now()
                root.moves = 0
                stepper.start()
                if (BUSY) root.spin()
            } else {
                console.log("ST " + root.steps + " " + (Date.now() - root.t0) + " " + root.moves)
                Qt.quit()
            }
        }
    }
}
"#;

/// What the fixture reported.
#[derive(Debug)]
struct Second {
    /// The steps of the animation driver: how many times it set the value of the animation.
    steps: u64,
    /// How long the counted second took, ms.
    ms: f64,
    /// Moves of the pointer the window was told of in that second, and calls of the chain.
    moves: u64,
}

/// Waits for the shell's line. A shell whose Timer never fires again is ended by nextest's
/// slow-timeout.
fn second(bin: &str, dir: &Path, busy: bool) -> Second {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("shell.qml"),
        SHELL.replace("BUSY", if busy { "true" } else { "false" }),
    )
    .unwrap();
    let (r, w) = std::io::pipe().unwrap();
    let mut c = Command::new(bin);
    c.arg("-p")
        .arg(dir)
        .env("RI_INTERP", "1")
        .env("QS_NO_RELOAD_POPUP", "1")
        .stdin(Stdio::null())
        .stdout(w.try_clone().unwrap())
        .stderr(w);
    let _k = Kill(c.spawn().unwrap());
    drop(c);
    let log = Log::capture(r);
    let line =
        |s: &str| (s.lines()).find_map(|l| l.find("ST ").map(|i| l[i + 3..].trim().to_string()));
    if let Err(s) = log.wait(|s| line(s).is_some()) {
        panic!("{bin}: exited before the second was counted; log:\n{s}");
    }
    let line = line(&log.text()).unwrap();
    let mut p = line.split(' ');
    let mut num = || p.next().unwrap().parse::<f64>().unwrap();
    Second {
        steps: num() as u64,
        ms: num(),
        moves: num() as u64,
    }
}

fn same_as_quickshell(moved: bool, busy: bool) {
    if !testenv::require("qs") {
        return;
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return;
    }
    let _moving = match moved.then(moving) {
        Some(None) => {
            testenv::unavailable("compositor lacks zwlr_virtual_pointer_v1");
            return;
        }
        m => m.flatten(),
    };
    let base = tempfile::tempdir().unwrap();
    // The pointer is the compositor's one: while another test's window is under it, this
    // one's sees no move, and the second is counted again.
    let counted = |bin: &str, dir: &str| {
        let count = || second(bin, &base.path().join(dir), busy);
        let mut s = count();
        for _ in 0..4 {
            if !moved || s.moves > 0 {
                break;
            }
            s = count();
        }
        s
    };
    let qt = counted("qs", "orig");
    let here = counted(env!("CARGO_BIN_EXE_rusty-impulse"), "native");
    println!("moved {moved} busy {busy}: Quickshell {qt:?}\nhere {here:?}");
    if moved {
        assert!(
            qt.moves > 0 && here.moves > 0,
            "the pointer moves: Quickshell saw {}, here {}",
            qt.moves,
            here.moves
        );
    }
    // Steps a second of the wall clock: no fewer than half of Quickshell's.
    let per_s = |s: &Second| s.steps as f64 * 1000.0 / s.ms;
    assert!(
        2.0 * per_s(&here) >= per_s(&qt),
        "{} steps in {} ms, under Quickshell {} steps in {} ms",
        here.steps,
        here.ms,
        qt.steps,
        qt.ms
    );
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn an_animation_that_draws_nothing_steps_once_per_vsync() {
    same_as_quickshell(false, false);
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn an_animation_that_draws_nothing_steps_once_per_vsync_while_the_pointer_moves() {
    same_as_quickshell(true, false);
}

/// `moves` counts the calls of a function that posts itself again with Qt.callLater here.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn an_animation_that_draws_nothing_steps_once_per_vsync_next_to_a_call_later_chain() {
    same_as_quickshell(false, true);
}
