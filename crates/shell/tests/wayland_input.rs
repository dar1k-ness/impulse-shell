//! Input that reaches impulse-shell only through Wayland protocols, on a wlroots compositor (the
//! testenv's sway): a held key repeats client-side from wl_keyboard.repeat_info (compositors
//! below wl_seat v10, Hyprland included, never send repeats themselves), and an input method's
//! text arrives through zwp_text_input_v3 like Qt's QWaylandTextInputv3, and a key pressed
//! with Ctrl on a non-Latin layout is released as the same Qt key it was pressed as; hover
//! motions queued behind a busy turn reach QML once, at the latest position, and those of a
//! held button each.

use std::io::Write;
use std::os::fd::AsFd;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use testenv::Log;
use testenv::wayland::{presented_after, turn_time};

use wayland_client::protocol::{wl_output, wl_pointer, wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, delegate_noop};
use wayland_protocols_misc::zwp_input_method_v2::client::{
    zwp_input_method_manager_v2::ZwpInputMethodManagerV2,
    zwp_input_method_v2::{self, ZwpInputMethodV2},
};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1,
    zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};

const SHELL: &str = r#"import QtQuick
import Rustshell
import Rustshell.Wayland
ShellRoot {
    PanelWindow {
        anchors.top: true
        anchors.left: true
        implicitWidth: 400
        implicitHeight: 100
        exclusionMode: ExclusionMode.Ignore
        WlrLayershell.keyboardFocus: WlrKeyboardFocus.Exclusive
        TextInput {
            anchors.fill: parent
            focus: true
            onTextChanged: console.log("TXT " + text)
            Component.onCompleted: console.log("TXT ready")
        }
    }
}
"#;

const KEYS_SHELL: &str = r#"import QtQuick
import Rustshell
import Rustshell.Wayland
ShellRoot {
    PanelWindow {
        anchors.top: true
        anchors.left: true
        implicitWidth: 400
        implicitHeight: 100
        exclusionMode: ExclusionMode.Ignore
        WlrLayershell.keyboardFocus: WlrKeyboardFocus.Exclusive
        Item {
            anchors.fill: parent
            focus: true
            Keys.onPressed: event => console.log("TXT p" + event.key)
            Keys.onReleased: event => console.log("TXT r" + event.key)
            Component.onCompleted: console.log("TXT ready")
        }
    }
}
"#;

/// Every hover move costs the handler ~2 ms, so motions pile up in the socket while it runs.
const MOTION_SHELL: &str = r#"import QtQuick
import Rustshell
ShellRoot {
    PanelWindow {
        anchors.top: true
        anchors.left: true
        implicitWidth: 400
        implicitHeight: 100
        exclusionMode: ExclusionMode.Ignore
        MouseArea {
            anchors.fill: parent
            hoverEnabled: true
            onPositionChanged: mouse => {
                const t = Date.now();
                while (Date.now() - t < 2) {}
                console.log("TXT m" + Math.round(mouse.x));
            }
            onPressed: console.log("TXT pressed")
            onReleased: console.log("TXT released")
            Component.onCompleted: console.log("TXT ready")
        }
    }
}
"#;

/// Three things a held pointer drags, one per row under a row that reports hover moves: an
/// item with a DragHandler, a Flickable and a SwipeView. Each logs where the drag put it.
const DRAG_SHELL: &str = r##"import QtQuick
import QtQuick.Controls
import Rustshell
ShellRoot {
    PanelWindow {
        anchors.top: true
        anchors.left: true
        implicitWidth: 400
        implicitHeight: 400
        exclusionMode: ExclusionMode.Ignore
        MouseArea {
            width: 400; height: 100
            hoverEnabled: true
            onPositionChanged: mouse => console.log("TXT m" + Math.round(mouse.x))
            Component.onCompleted: console.log("TXT ready")
        }
        Rectangle {
            x: 270; y: 120; width: 60; height: 60
            color: "#353"
            onXChanged: console.log("TXT item " + Math.round(x))
            DragHandler {
                yAxis.enabled: false
                onActiveChanged: console.log("TXT handler " + active)
            }
        }
        Flickable {
            y: 200; width: 400; height: 100
            contentWidth: 1200; contentHeight: 100
            onContentXChanged: if (dragging) console.log("TXT flick " + Math.round(contentX))
            onDraggingChanged: console.log("TXT flick dragging " + dragging)
            Rectangle { width: 1200; height: 100; color: "#335" }
        }
        SwipeView {
            id: swipe
            y: 300; width: 400; height: 100
            property real seen: 0
            onCurrentIndexChanged: console.log("TXT page " + currentIndex)
            Rectangle { id: page0; color: "#533" }
            Rectangle { color: "#355" }
            Rectangle { color: "#553" }
        }
        FrameAnimation {
            running: true
            onTriggered: {
                const o = Math.round(-page0.mapToItem(swipe, 0, 0).x);
                if (o !== swipe.seen)
                    console.log("TXT swipe " + o);
                swipe.seen = o;
            }
        }
    }
}
"##;

/// Things a held pointer drags whose press handler takes 120 ms, twice each (one per column),
/// under a row that reports hover moves and takes 120 ms to handle a release: a MouseArea
/// that is its own `drag.target`, an item with a DragHandler and a MouseArea inside, and a
/// Flickable with a MouseArea inside. Each logs how many moves it saw and where it ended.
const HELD_SHELL: &str = r##"import QtQuick
import Rustshell
ShellRoot {
    PanelWindow {
        id: win
        anchors.top: true
        anchors.left: true
        implicitWidth: 800
        implicitHeight: 400
        exclusionMode: ExclusionMode.Ignore
        function busy(what) {
            console.log("TXT " + what);
            const t = Date.now();
            while (Date.now() - t < 120) {}
        }
        MouseArea {
            width: 800; height: 100
            hoverEnabled: true
            onPositionChanged: mouse => console.log("TXT m" + Math.round(mouse.x))
            onReleased: win.busy("released")
            Component.onCompleted: console.log("TXT ready")
        }
        MouseArea {
            id: a0
            x: 20; y: 120; width: 60; height: 60
            drag.target: a0
            onPressed: win.busy("a pressed")
            onReleased: console.log("TXT a end " + x + "," + y)
        }
        MouseArea {
            id: a1
            x: 420; y: 120; width: 60; height: 60
            drag.target: a1
            onPressed: win.busy("a pressed")
            onReleased: console.log("TXT a end " + x + "," + y)
        }
        Rectangle {
            id: b0
            x: 20; y: 220; width: 60; height: 60
            color: "#353"
            DragHandler {
                yAxis.enabled: false
                onActiveChanged: if (!active) console.log("TXT b end " + Math.round(b0.x))
            }
            MouseArea {
                anchors.fill: parent
                onPressed: win.busy("b pressed")
            }
        }
        Rectangle {
            id: b1
            x: 420; y: 220; width: 60; height: 60
            color: "#353"
            DragHandler {
                yAxis.enabled: false
                onActiveChanged: if (!active) console.log("TXT b end " + Math.round(b1.x))
            }
            MouseArea {
                anchors.fill: parent
                onPressed: win.busy("b pressed")
            }
        }
        Flickable {
            y: 300; width: 400; height: 100
            contentWidth: 1200; contentHeight: 100
            onDraggingChanged: if (!dragging) console.log("TXT c end")
            MouseArea {
                width: 1200; height: 100
                onPressed: win.busy("c pressed")
            }
        }
        Flickable {
            x: 400; y: 300; width: 400; height: 100
            contentWidth: 1200; contentHeight: 100
            onDraggingChanged: if (!dragging) console.log("TXT c end")
            MouseArea {
                width: 1200; height: 100
                onPressed: win.busy("c pressed")
            }
        }
    }
}
"##;

/// The tests' exclusive-keyboard panels would steal each other's focus.
static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Default)]
struct G {
    seat: Option<wl_seat::WlSeat>,
    vkbd: Option<ZwpVirtualKeyboardManagerV1>,
    im: Option<ZwpInputMethodManagerV2>,
    vptr: Option<ZwlrVirtualPointerManagerV1>,
    output: Option<wl_output::WlOutput>,
    mode: (u32, u32),
    active: bool,
    pending_active: bool,
    serial: u32,
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
                "zwp_virtual_keyboard_manager_v1" => g.vkbd = Some(reg.bind(name, 1, qh, ())),
                "zwp_input_method_manager_v2" => g.im = Some(reg.bind(name, 1, qh, ())),
                "zwlr_virtual_pointer_manager_v1" => {
                    g.vptr = Some(reg.bind(name, version.min(2), qh, ()))
                }
                "wl_output" if g.output.is_none() => g.output = Some(reg.bind(name, 1, qh, ())),
                _ => {}
            }
        }
    }
}

impl Dispatch<ZwpInputMethodV2, ()> for G {
    fn event(
        g: &mut Self,
        _: &ZwpInputMethodV2,
        ev: zwp_input_method_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match ev {
            zwp_input_method_v2::Event::Activate => g.pending_active = true,
            zwp_input_method_v2::Event::Deactivate => g.pending_active = false,
            zwp_input_method_v2::Event::Done => {
                g.active = g.pending_active;
                g.serial += 1;
            }
            _ => {}
        }
    }
}

delegate_noop!(G: ignore wl_seat::WlSeat);
delegate_noop!(G: ZwpVirtualKeyboardManagerV1);
delegate_noop!(G: ZwpVirtualKeyboardV1);
delegate_noop!(G: ZwpInputMethodManagerV2);
delegate_noop!(G: ZwlrVirtualPointerManagerV1);
delegate_noop!(G: ZwlrVirtualPointerV1);

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

struct Shell {
    child: Child,
    log: Arc<Log>,
}

impl Shell {
    fn spawn(dir: &Path, qml: &str) -> Self {
        std::fs::write(dir.join("shell.qml"), qml).unwrap();
        for d in ["config", "state", "cache"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        let (r, w) = std::io::pipe().unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_impulse-shell"))
            .arg("-p")
            .arg(dir)
            .env("RI_INTERP", "1")
            .env("WAYLAND_DEBUG", "1")
            .env(
                "RI_LOG",
                "ri::present=debug,ri::sched=debug,ri::keys=debug,ri::ptr=trace",
            )
            .env("XDG_CONFIG_HOME", dir.join("config"))
            .env("XDG_STATE_HOME", dir.join("state"))
            .env("XDG_CACHE_HOME", dir.join("cache"))
            .stdout(w.try_clone().unwrap())
            .stderr(w)
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        Shell {
            child,
            log: Log::capture(r),
        }
    }

    fn texts(&self) -> Vec<String> {
        texts(&self.log.text())
    }

    /// Waits for `f(log)`; panics if impulse-shell exits first.
    fn wait_log(&self, what: &str, f: impl Fn(&str) -> bool) -> String {
        if let Err(s) = self.log.wait(&f) {
            let tail: Vec<&str> = s
                .lines()
                .filter(|l| l.contains("text_input") || l.contains("keyboard") || l.contains("TXT"))
                .collect();
            panic!("impulse-shell exited before {what}:\n{}", tail.join("\n"));
        }
        self.log.text()
    }

    /// Waits until QML's last line is `text` and the turn that printed it has ended: what
    /// is sent next is not read together with what that turn read.
    fn settled(&self, text: &str) {
        self.wait_log(text, |s| {
            texts(s).last().is_some_and(|l| l == text)
                && turned_after(s, |l| l.split_once("TXT ").is_some_and(|(_, t)| t == text))
        });
    }

    /// Waits until the last `TXT` line satisfies `ok`, and returns it.
    fn wait(&self, what: &str, ok: impl Fn(&str) -> bool) -> String {
        let last = |s: &str| texts(s).pop().filter(|t| ok(t));
        last(&self.wait_log(what, |s| last(s).is_some())).unwrap()
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// What QML printed after `TXT `, in order.
fn texts(log: &str) -> Vec<String> {
    log.lines()
        .filter_map(|l| l.split_once("TXT ").map(|(_, t)| t.to_string()))
        .collect()
}

/// The loop finished a turn (`RI_LOG=ri::sched` logs each as `t=… next …`) after the last line
/// `mark` matches: what the shell read with that line is handled, and what it reads next
/// comes out of another queue.
fn turned_after(log: &str, mark: impl Fn(&str) -> bool) -> bool {
    let lines: Vec<&str> = log.lines().collect();
    lines
        .iter()
        .rposition(|l| mark(l))
        .is_some_and(|m| lines[m + 1..].iter().any(|l| turn_time(l).is_some()))
}

/// Dispatches the compositor's events to `g` until `ok(g)`.
fn dispatch_until(q: &mut EventQueue<G>, g: &mut G, ok: impl Fn(&G) -> bool) {
    while !ok(g) {
        q.blocking_dispatch(g).unwrap();
    }
}

/// A zwp_virtual_keyboard_v1 on the seat with the xkb `layout` keymap uploaded.
fn virtual_keyboard(
    q: &mut EventQueue<G>,
    g: &mut G,
    layout: &str,
) -> Option<(wl_seat::WlSeat, ZwpVirtualKeyboardV1)> {
    let qh = q.handle();
    let (seat, vkbd) = (g.seat.clone()?, g.vkbd.clone()?);
    let kbd = vkbd.create_virtual_keyboard(&seat, &qh, ());
    let ctx = xkbcommon::xkb::Context::new(xkbcommon::xkb::CONTEXT_NO_FLAGS);
    let km = xkbcommon::xkb::Keymap::new_from_names(
        &ctx,
        "",
        "",
        layout,
        "",
        None,
        xkbcommon::xkb::KEYMAP_COMPILE_NO_FLAGS,
    )
    .unwrap();
    let text = km.get_as_string(xkbcommon::xkb::KEYMAP_FORMAT_TEXT_V1);
    let mut keymap = tempfile::tempfile().unwrap();
    keymap.write_all(text.as_bytes()).unwrap();
    keymap.write_all(&[0]).unwrap();
    kbd.keymap(1, keymap.as_fd(), text.len() as u32 + 1);
    q.roundtrip(g).unwrap();
    Some((seat, kbd))
}

/// Ctrl+A on `ru` (QXkbCommon's Latin fallback makes the press Qt::Key_A, not Ф): the release
/// is Key_A too, whether A or Ctrl is let go first, so `Keys.onReleased` sees the key
/// `onPressed` saw.
#[test]
fn ctrl_key_on_non_latin_layout_releases_as_pressed() {
    let _l = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Ok(conn) = Connection::connect_to_env() else {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return;
    };
    let mut q = conn.new_event_queue();
    conn.display().get_registry(&q.handle(), ());
    let mut g = G::default();
    q.roundtrip(&mut g).unwrap();
    let Some((_seat, kbd)) = virtual_keyboard(&mut q, &mut g, "us,ru") else {
        testenv::unavailable("compositor lacks zwp_virtual_keyboard_v1");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let sh = Shell::spawn(dir.path(), KEYS_SHELL);
    sh.wait("the shell never loaded", |t| t == "ready");
    // Second layout (ru) locked.
    kbd.modifiers(0, 0, 0, 1);
    conn.flush().unwrap();
    // The exclusive layer surface takes keyboard focus once it is mapped; keys reach it once
    // that activation took effect (the wl_display.sync after wl_keyboard.enter answered).
    sh.wait_log("the panel's keyboard activation", |s| {
        s.contains("kbd active Some")
    });
    // A Shift tap: a key that reaches the panel, without text.
    const SHIFT: [&str; 2] = ["p16777248", "r16777248"];
    let shift_taps = |s: &str| texts(s).iter().filter(|t| *t == SHIFT[1]).count();
    kbd.key(0, 42, 1);
    kbd.key(0, 42, 0);
    conn.flush().unwrap();
    sh.wait_log("the Shift tap reaching QML", |s| shift_taps(s) == 1);
    let start = sh.texts().len();
    const CTRL: u32 = 4;
    // A released under Ctrl.
    kbd.modifiers(CTRL, 0, 0, 1);
    kbd.key(10, 30, 1);
    kbd.key(20, 30, 0);
    kbd.modifiers(0, 0, 0, 1);
    // Ctrl released before A.
    kbd.modifiers(CTRL, 0, 0, 1);
    kbd.key(30, 30, 1);
    kbd.modifiers(0, 0, 0, 1);
    kbd.key(40, 30, 0);
    // A second Shift tap: once QML saw it, it saw every key event sent before it.
    kbd.key(50, 42, 1);
    kbd.key(60, 42, 0);
    conn.flush().unwrap();
    let s = sh.wait_log("the second Shift tap reaching QML", |s| shift_taps(s) == 2);
    assert_eq!(
        texts(&s)[start..],
        ["p65", "r65", "p65", "r65", SHIFT[0], SHIFT[1]]
    );
}

#[test]
fn held_key_repeats_and_input_method_commits() {
    let _l = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Ok(conn) = Connection::connect_to_env() else {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return;
    };
    let mut q = conn.new_event_queue();
    let qh = q.handle();
    conn.display().get_registry(&qh, ());
    let mut g = G::default();
    q.roundtrip(&mut g).unwrap();
    let (Some(seat), Some(vkbd), Some(imm)) = (g.seat.clone(), g.vkbd.clone(), g.im.clone()) else {
        testenv::unavailable("compositor lacks zwp_virtual_keyboard_v1/zwp_input_method_v2");
        return;
    };
    let im = imm.get_input_method(&seat, &qh, ());
    let kbd = vkbd.create_virtual_keyboard(&seat, &qh, ());
    let ctx = xkbcommon::xkb::Context::new(xkbcommon::xkb::CONTEXT_NO_FLAGS);
    let km = xkbcommon::xkb::Keymap::new_from_names(
        &ctx,
        "",
        "",
        "us",
        "",
        None,
        xkbcommon::xkb::KEYMAP_COMPILE_NO_FLAGS,
    )
    .unwrap();
    let text = km.get_as_string(xkbcommon::xkb::KEYMAP_FORMAT_TEXT_V1);
    let mut keymap = tempfile::tempfile().unwrap();
    keymap.write_all(text.as_bytes()).unwrap();
    keymap.write_all(&[0]).unwrap();
    kbd.keymap(1, keymap.as_fd(), text.len() as u32 + 1);
    q.roundtrip(&mut g).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let sh = Shell::spawn(dir.path(), SHELL);
    sh.wait("the shell never loaded", |t| t == "ready");
    // The exclusive layer surface takes keyboard focus once it is mapped and has drawn.
    dispatch_until(&mut q, &mut g, |g| g.active);

    // Held until the shell typed it five more times from sway's wl_keyboard.repeat_info.
    kbd.key(0, 30, 1);
    conn.flush().unwrap();
    sh.wait("the held key repeating", |t| t.len() >= 6);
    kbd.key(1, 30, 0);
    conn.flush().unwrap();
    // The line where the shell's main thread took the release (ri::keys logs each key it
    // delivers; a repeat's release and press carry the text of the held key, the release of
    // the key itself none). Not the `wl_keyboard.key` line of WAYLAND_DEBUG: libwayland prints
    // that on the shell's reader thread as the event arrives, and a repeat the main thread
    // is typing then, before it has seen the release, is no repeat that outlived it.
    let released = |s: &str| {
        s.lines().position(|l| {
            l.starts_with("key sym=") && l.contains(" text=\"\" ") && l.contains(" press=false ")
        })
    };
    let s = sh.wait_log("the release", |s| released(s).is_some());
    let rate: f64 = s
        .lines()
        .find_map(|l| {
            l.split_once(".repeat_info(")?
                .1
                .split(',')
                .next()?
                .parse()
                .ok()
        })
        .expect("no wl_keyboard.repeat_info");
    assert!(rate > 0.0, "sway sent repeat_info rate {rate}");
    // The loop's own clock (ri::sched turn times; the caret blink keeps it turning) two repeat
    // intervals past the turn that handled the release: a repeat still armed has fired by then.
    let turns_after = |s: &str| -> Vec<f64> {
        let i = released(s).unwrap();
        s.lines().skip(i).filter_map(turn_time).collect()
    };
    let s = sh.wait_log("two repeat intervals after the release", |s| {
        let t = turns_after(s);
        t.first()
            .is_some_and(|&t0| t.last().is_some_and(|&t1| t1 >= t0 + 2000.0 / rate))
    });
    let i = released(&s).unwrap();
    let before = s.lines().take(i).collect::<Vec<_>>().join("\n");
    let after = s.lines().skip(i).collect::<Vec<_>>().join("\n");
    let typed = texts(&before).pop().unwrap();
    assert!(
        typed.len() >= 6 && typed.chars().all(|c| c == 'a'),
        "holding a key typed {typed:?}"
    );
    assert_eq!(
        texts(&after),
        Vec::<String>::new(),
        "repeat outlived the release"
    );

    q.roundtrip(&mut g).unwrap();
    im.commit_string("привет".into());
    im.commit(g.serial);
    conn.flush().unwrap();
    let want = format!("{typed}привет");
    sh.wait("the input method's commit never arrived", |t| t == want);
}

/// A virtual pointer hovering MOTION_SHELL's panel at (20, 50).
struct Hovering {
    conn: Connection,
    ptr: ZwlrVirtualPointerV1,
    sh: Shell,
    /// Time of the last event sent.
    t: u32,
    mode: (u32, u32),
    _dir: tempfile::TempDir,
}

fn pointer_over_panel() -> Option<Hovering> {
    pointer_over(MOTION_SHELL)
}

/// The pointer over the top row of `qml`'s panel, whose MouseArea logs `m<x>` for each move.
fn pointer_over(qml: &str) -> Option<Hovering> {
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
    assert!(w >= 400 && h >= 100, "first output mode {:?}", g.mode);
    let dir = tempfile::tempdir().unwrap();
    let sh = Shell::spawn(dir.path(), qml);
    sh.wait("the shell never loaded", |t| t == "ready");
    // The panel on screen, the pointer over it (wl_pointer.enter), then moved within it.
    sh.wait_log("the panel on screen", |s| {
        presented_after(s, |l| l.contains("TXT ready"))
    });
    let entered = |s: &str| {
        s.lines()
            .any(|l| l.contains("wl_pointer#") && !l.contains(" -> ") && l.contains(".enter("))
    };
    let mut t = 10;
    ptr.motion_absolute(t, 10, 50, w, h);
    ptr.frame();
    conn.flush().unwrap();
    sh.wait_log("the pointer entering the panel", entered);
    t += 10;
    ptr.motion_absolute(t, 20, 50, w, h);
    ptr.frame();
    conn.flush().unwrap();
    sh.settled("m20");
    Some(Hovering {
        conn,
        ptr,
        sh,
        t,
        mode: g.mode,
        _dir: dir,
    })
}

/// 200 hover moves sent in one burst while each `positionChanged` handler takes ~2 ms: the
/// shell delivers the motions it finds queued as one move to the latest position, so the
/// handler runs far fewer than 200 times and its last call sees where the pointer ended.
#[test]
fn queued_motions_reach_qml_once_at_the_latest_position() {
    let _l = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(Hovering {
        conn,
        ptr,
        sh,
        mut t,
        mode: (w, h),
        _dir,
    }) = pointer_over_panel()
    else {
        return;
    };
    let start = sh.texts().len();
    let cpu0 = cpu_ms(sh.child.id());
    const MOVES: u32 = 200;
    for i in 0..MOVES {
        t += 1;
        ptr.motion_absolute(t, 100 + i, 50, w, h);
        ptr.frame();
    }
    conn.flush().unwrap();
    let last = format!("m{}", 100 + MOVES - 1);
    sh.wait("the last motion never reached QML", |l| l == last);
    let cpu = cpu_ms(sh.child.id()) - cpu0;
    // One more motion: QML sees it only after every motion queued before it.
    ptr.motion_absolute(t + 1, 350, 50, w, h);
    ptr.frame();
    conn.flush().unwrap();
    sh.wait("the motion after the burst never reached QML", |l| {
        l == "m350"
    });
    let got = sh.texts();
    let burst = &got[start..got.len() - 1];
    let calls = burst.iter().filter(|l| l.starts_with('m')).count();
    eprintln!("{MOVES} queued motions: {calls} positionChanged, {cpu:.1} ms CPU");
    assert_eq!(
        burst.last().unwrap(),
        &last,
        "a stale motion arrived after the latest"
    );
    assert!(
        calls < MOVES as usize / 4,
        "{calls} of {MOVES} queued motions reached QML"
    );
}

/// Bursts of 20 moves, each read from the socket in one go, reach QML as Qt 6.12 delivers
/// them (`QWaylandInputDevice::Pointer::pointer_frame`): the first pointer frame of a burst
/// is handled at once, every later move replaces the one waiting, and the one waiting is
/// delivered before a button and when the burst ends. With a button held or not.
#[test]
fn a_burst_of_motions_reaches_qml_as_first_and_last() {
    let _l = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(Hovering {
        conn,
        ptr,
        sh,
        mut t,
        mode: (w, h),
        _dir,
    }) = pointer_over_panel()
    else {
        return;
    };
    const BTN_LEFT: u32 = 0x110;
    let burst = |t: &mut u32, from: u32| {
        for i in 0..20 {
            *t += 1;
            ptr.motion_absolute(*t, from + i, 50, w, h);
            ptr.frame();
        }
    };
    let button = |t: &mut u32, state| {
        *t += 1;
        ptr.button(*t, BTN_LEFT, state);
        ptr.frame();
    };
    let start = sh.texts().len();
    // No button.
    burst(&mut t, 100);
    conn.flush().unwrap();
    sh.settled("m119");
    // The button already down when the burst comes.
    button(&mut t, wl_pointer::ButtonState::Pressed);
    conn.flush().unwrap();
    sh.settled("pressed");
    burst(&mut t, 200);
    conn.flush().unwrap();
    sh.settled("m219");
    button(&mut t, wl_pointer::ButtonState::Released);
    conn.flush().unwrap();
    sh.settled("released");
    // Press, burst and release together: the press is the burst's first frame.
    button(&mut t, wl_pointer::ButtonState::Pressed);
    burst(&mut t, 300);
    button(&mut t, wl_pointer::ButtonState::Released);
    conn.flush().unwrap();
    sh.wait_log("the second release never reached QML", |s| {
        texts(s).iter().filter(|l| *l == "released").count() == 2
    });
    assert_eq!(
        sh.texts()[start..],
        [
            "m100", "m119", "pressed", "m200", "m219", "released", "pressed", "m319", "released"
        ]
    );
}

/// A press, a move of 20 px that starts the drag, then 20 moves of 5 px read from the socket in
/// one go, on a DragHandler's item, a Flickable and a SwipeView: each sees the burst's first
/// and last move, as in Qt 6.12. The item goes to both; a Flickable and a SwipeView's list
/// start from the move after the one that crossed the threshold, the burst's first, and
/// follow the last by 95 px.
#[test]
fn a_burst_of_motions_drags_as_in_qt() {
    let _l = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(Hovering {
        conn,
        ptr,
        sh,
        mut t,
        mode: (w, h),
        _dir,
    }) = pointer_over(DRAG_SHELL)
    else {
        return;
    };
    const BTN_LEFT: u32 = 0x110;
    let buttons = |s: &str| {
        s.lines()
            .filter(|l| l.contains("wl_pointer#") && !l.contains(" -> ") && l.contains(".button("))
            .count()
    };
    let motions = |s: &str| {
        s.lines()
            .filter(|l| l.contains("wl_pointer#") && !l.contains(" -> ") && l.contains(".motion("))
            .count()
    };
    let mut got = Vec::new();
    for (row, y) in [150, 250, 350].into_iter().enumerate() {
        let start = sh.texts().len();
        t += 10;
        ptr.motion_absolute(t, 300, y, w, h);
        ptr.frame();
        t += 10;
        ptr.button(t, BTN_LEFT, wl_pointer::ButtonState::Pressed);
        ptr.frame();
        conn.flush().unwrap();
        let pressed = sh.wait_log("the press", |s| {
            buttons(s) == 2 * row + 1 && turned_after(s, |l| l.contains(".button("))
        });
        // One move over the drag threshold, alone: the drag has begun when the burst comes.
        t += 10;
        ptr.motion_absolute(t, 280, y, w, h);
        ptr.frame();
        conn.flush().unwrap();
        sh.wait_log("the move over the threshold", |s| {
            motions(s) == motions(&pressed) + 1 && turned_after(s, |l| l.contains(".motion("))
        });
        for i in 1..=20 {
            t += 1;
            ptr.motion_absolute(t, 280 - 5 * i, y, w, h);
            ptr.frame();
        }
        conn.flush().unwrap();
        sh.wait_log("the burst", |s| {
            motions(s) == motions(&pressed) + 21 && turned_after(s, |l| l.contains(".motion("))
        });
        if row == 2 {
            // The SwipeView's pages are read once a frame.
            sh.wait_log("a frame with the pages moved", |s| {
                texts(s)[start..].iter().any(|l| l.starts_with("swipe "))
            });
        }
        let burst = sh.texts().len();
        t += 1;
        ptr.button(t, BTN_LEFT, wl_pointer::ButtonState::Released);
        ptr.frame();
        // Back over the top row: its move is logged after everything the drag logged.
        let back = format!("m{}", 30 + row);
        t += 10;
        ptr.motion_absolute(t, 30 + row as u32, 50, w, h);
        ptr.frame();
        conn.flush().unwrap();
        let seen = texts(&sh.wait_log("the pointer back over the top row", |s| {
            texts(s).contains(&back)
        }));
        // The frames in which the pages settle after the release are not events.
        let till = seen.iter().position(|l| *l == back).unwrap();
        let during = seen[start..burst].iter();
        let after = seen[burst..=till]
            .iter()
            .filter(|l| !l.starts_with("swipe "));
        let seen: Vec<&str> = during.chain(after).map(|l| l.as_str()).collect();
        got.push(seen.join(", "));
    }
    assert_eq!(
        got,
        [
            "handler true, item 250, item 245, item 150, handler false, m30",
            "flick dragging true, flick 95, flick dragging false, m31",
            "swipe 95, m32"
        ]
    );
}

/// Moves that come while a press handler runs for 120 ms reach QML as in Qt 6.12, which
/// handles the press after it has read the socket and reads the moves in its next pass: the
/// first of them is delivered at once and starts the drag, the rest replace the one waiting.
/// 20 moves of 15 px behind the press in one go, with the release (left column), and 4 behind
/// the press, then 16 one by one (right column), for a MouseArea that is its own
/// `drag.target`, a DragHandler's item with a MouseArea in it and a Flickable with a
/// MouseArea in it (which takes the pointer on the first move and starts from the second).
/// Then a release whose handler takes 120 ms, with 20 hover moves behind it.
///
/// How many of the moves in between reach QML depends on the clock, in Qt as well (two frames
/// of one read more than 100 µs apart on a loaded machine are both delivered; `input.rs`
/// tests that rule with given times): where the drag ends, that the Flickable was dragged at
/// all, and the first and the last hover move do not.
#[test]
fn motions_behind_a_long_handler_reach_qml_as_in_qt() {
    let _l = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(Hovering {
        conn,
        ptr,
        sh,
        mut t,
        mode: (w, h),
        _dir,
    }) = pointer_over(HELD_SHELL)
    else {
        return;
    };
    const BTN_LEFT: u32 = 0x110;
    let mut got = Vec::new();
    let mut back = 30;
    // The pointer goes right over the MouseArea and the item, left over the Flickable.
    for (row, y, from, step) in [("a", 150, 50, 15), ("b", 250, 50, 15), ("c", 350, 350, -15)] {
        for steps in [false, true] {
            let x = |k: i32| (from + if steps { 400 } else { 0 } + step * k) as u32;
            let start = sh.texts().len();
            t += 10;
            ptr.motion_absolute(t, x(0), y, w, h);
            ptr.frame();
            t += 10;
            ptr.button(t, BTN_LEFT, wl_pointer::ButtonState::Pressed);
            ptr.frame();
            conn.flush().unwrap();
            // The handler has logged the press and is busy: what is sent now waits behind it.
            let pressed = format!("{row} pressed");
            sh.wait_log("the press handler", |s| {
                texts(s)[start..].contains(&pressed)
            });
            let behind = if steps { 4 } else { 20 };
            for k in 1..=behind {
                t += 1;
                ptr.motion_absolute(t, x(k), y, w, h);
                ptr.frame();
            }
            if steps {
                conn.flush().unwrap();
                for k in behind..=20 {
                    // `RI_LOG=ri::ptr=trace` logs a move when the shell handles it; the
                    // socket is read, and WAYLAND_DEBUG prints, while the handler still runs.
                    let handled = format!("Motion {{ time: {t} }}");
                    sh.wait_log("a move", |s| turned_after(s, |l| l.contains(&handled)));
                    if k < 20 {
                        t += 16;
                        ptr.motion_absolute(t, x(k + 1), y, w, h);
                        ptr.frame();
                        conn.flush().unwrap();
                    }
                }
            }
            t += 1;
            ptr.button(t, BTN_LEFT, wl_pointer::ButtonState::Released);
            ptr.frame();
            // Back over the top row: its move is logged after everything the drag logged.
            back += 1;
            t += 10;
            ptr.motion_absolute(t, back, 50, w, h);
            ptr.frame();
            conn.flush().unwrap();
            let mark = format!("m{back}");
            sh.settled(&mark);
            let seen = sh.texts();
            got.push(seen[start..seen.len() - 1].join(", "));
        }
    }
    // A press and a release over the top row, then 20 moves behind the release handler.
    let start = sh.texts().len();
    for state in [
        wl_pointer::ButtonState::Pressed,
        wl_pointer::ButtonState::Released,
    ] {
        t += 10;
        ptr.button(t, BTN_LEFT, state);
        ptr.frame();
    }
    conn.flush().unwrap();
    sh.wait("the release handler", |l| l == "released");
    for i in 0..20 {
        t += 1;
        ptr.motion_absolute(t, 100 + i, 50, w, h);
        ptr.frame();
    }
    conn.flush().unwrap();
    sh.settled("m119");
    let hover = sh.texts()[start..].to_vec();
    let at = |t: &String| t.strip_prefix('m').and_then(|x| x.parse::<u32>().ok());
    assert!(
        hover[0] == "released" && hover[1..].iter().map(at).is_sorted_by(|a, b| a < b),
        "{hover:?}"
    );
    got.push([&hover[..2], &hover[hover.len() - 1..]].concat().join(", "));
    assert_eq!(
        got,
        [
            "a pressed, a end 305,120",
            "a pressed, a end 705,120",
            "b pressed, b end 320",
            "b pressed, b end 720",
            "c pressed, c end",
            "c pressed, c end",
            "released, m100, m119"
        ]
    );
}

/// Time `pid` spent on a CPU, in ms.
fn cpu_ms(pid: u32) -> f64 {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/schedstat")).unwrap();
    stat.split_whitespace()
        .next()
        .unwrap()
        .parse::<f64>()
        .unwrap()
        / 1e6
}
