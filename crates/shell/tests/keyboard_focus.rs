//! Keyboard focus belongs to a wl_surface (wl_keyboard.enter/leave name one): a window closed
//! while it holds the focus is deactivated with its surface, so when the same window is shown
//! again on a new surface the compositor never focused, it does not get the keys. That surface
//! asks for no keyboard (`WlrLayershell.keyboardFocus: None`), so the compositor never enters it.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::AsFd;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};

use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, QueueHandle, delegate_noop};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1,
    zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
};

/// An Exclusive panel takes the keyboard; once its field has active focus it hides, and once
/// that focus is gone it shows again with keyboardFocus None, on a new layer surface that asks
/// for no keyboard: the compositor never enters it.
const SHELL: &str = r#"import QtQuick
import QtQuick.Controls
import Rustshell
import Rustshell.Wayland
ShellRoot {
    id: root
    property bool shown: true
    property bool reopened: false
    PanelWindow {
        visible: root.shown
        WlrLayershell.namespace: "kbdfocus"
        WlrLayershell.keyboardFocus: root.reopened ? WlrKeyboardFocus.None : WlrKeyboardFocus.Exclusive
        anchors { top: true; left: true; right: true }
        implicitHeight: 100
        TextField {
            focus: true
            onActiveFocusChanged: {
                console.log("TXT focus " + activeFocus);
                if (activeFocus && !root.reopened) {
                    root.shown = false;
                } else if (!activeFocus && !root.shown) {
                    root.reopened = true;
                    root.shown = true;
                }
            }
        }
    }
}
"#;

#[derive(Default)]
struct G {
    seat: Option<wl_seat::WlSeat>,
    vkbd: Option<ZwpVirtualKeyboardManagerV1>,
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
            name, interface, ..
        } = ev
        {
            match interface.as_str() {
                "wl_seat" if g.seat.is_none() => g.seat = Some(reg.bind(name, 1, qh, ())),
                "zwp_virtual_keyboard_manager_v1" => g.vkbd = Some(reg.bind(name, 1, qh, ())),
                _ => {}
            }
        }
    }
}

delegate_noop!(G: ignore wl_seat::WlSeat);
delegate_noop!(G: ZwpVirtualKeyboardManagerV1);
delegate_noop!(G: ZwpVirtualKeyboardV1);

/// The shell's output, filled by a reader thread that wakes waiters on every line.
#[derive(Default)]
struct Log {
    /// The output so far, and whether the shell closed it (exited).
    text: Mutex<(String, bool)>,
    grew: Condvar,
}

impl Log {
    fn capture(r: impl Read + Send + 'static) -> Arc<Self> {
        let log = Arc::new(Log::default());
        let l = log.clone();
        std::thread::spawn(move || {
            let mut r = BufReader::new(r);
            let mut line = Vec::new();
            loop {
                line.clear();
                let n = r.read_until(b'\n', &mut line).unwrap_or(0);
                let mut g = l.text.lock().unwrap();
                g.0.push_str(&String::from_utf8_lossy(&line));
                g.1 = n == 0;
                drop(g);
                l.grew.notify_all();
                if n == 0 {
                    return;
                }
            }
        });
        log
    }

    /// Blocks until the shell's output lines satisfy `f` and returns them; panics if it exits
    /// first.
    fn wait(&self, what: &str, f: impl Fn(&[&str]) -> bool) -> Vec<String> {
        let mut g = self.text.lock().unwrap();
        loop {
            let lines: Vec<&str> = g.0.lines().collect();
            if f(&lines) {
                return lines.into_iter().map(str::to_owned).collect();
            }
            if g.1 {
                let msg = format!("the shell exited before {what}; it printed {lines:#?}");
                drop(g);
                panic!("{msg}");
            }
            g = self.grew.wait(g).unwrap();
        }
    }
}

fn texts<'a>(lines: &[&'a str]) -> Vec<&'a str> {
    lines
        .iter()
        .filter_map(|l| l.split_once("TXT ").map(|(_, t)| t))
        .collect()
}

struct Shell(Child);

impl Drop for Shell {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Before the fix keyboard focus was keyed by the window item and nothing cleared it when the
/// window closed, so the reopened panel's new surface was treated as focused before (and
/// without) any wl_keyboard.enter: its field got active focus again.
#[test]
fn closed_window_keeps_no_keyboard_focus_for_its_next_surface() {
    let Ok(conn) = Connection::connect_to_env() else {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return;
    };
    let mut q = conn.new_event_queue();
    let qh = q.handle();
    conn.display().get_registry(&qh, ());
    let mut g = G::default();
    q.roundtrip(&mut g).unwrap();
    let (Some(seat), Some(vkbd)) = (g.seat.clone(), g.vkbd.clone()) else {
        testenv::unavailable("compositor lacks zwp_virtual_keyboard_v1");
        return;
    };
    // The headless seat has no keyboard until a (virtual) one with a keymap joins it.
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
    std::fs::write(dir.path().join("shell.qml"), SHELL).unwrap();
    let xdg = dir.path().join("xdg");
    let (r, out) = std::io::pipe().unwrap();
    let sh = Shell(
        Command::new(env!("CARGO_BIN_EXE_impulse-shell"))
            .arg("-p")
            .arg(dir.path())
            .env("RI_INTERP", "1")
            .env("RI_LOG", "ri::frames=debug,ri::keys=debug")
            .env("WAYLAND_DEBUG", "1")
            .env("XDG_CONFIG_HOME", xdg.join("config"))
            .env("XDG_STATE_HOME", xdg.join("state"))
            .env("XDG_CACHE_HOME", xdg.join("cache"))
            .stdout(out.try_clone().unwrap())
            .stderr(out)
            .stdin(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let log = Log::capture(r);

    log.wait("the Exclusive panel's keyboard focus", |l| {
        texts(l).starts_with(&["focus true"])
    });
    log.wait("the focus to end with the closed panel's surface", |l| {
        texts(l).starts_with(&["focus true", "focus false"])
    });
    // The reopened panel's first frame: it was retargeted many times before it could draw.
    let lines = log.wait("the reopened panel's first frame", |l| {
        l.iter()
            .skip_while(|l| !l.contains("TXT focus false"))
            .any(|l| l.contains("draw win"))
    });
    let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
    // Quickshell: `focusable` follows keyboardFocus, so Exclusive then None asks for None. With
    // OnDemand (what `focusable` left true by Exclusive asked for) sway enters the new surface
    // right after its first frame.
    let asked: Vec<&str> = lines
        .iter()
        .filter_map(|l| l.split_once(".set_keyboard_interactivity("))
        .map(|(_, m)| m.trim_end_matches(')'))
        .collect();
    assert_eq!(
        asked,
        ["1", "0"],
        "the keyboard modes of the two layer surfaces (1 Exclusive, 0 None)"
    );
    assert_eq!(
        texts(&lines),
        ["focus true", "focus false"],
        "a surface the compositor never entered got the keyboard: {lines:#?}"
    );
    drop(sh);
    kbd.destroy();
    q.roundtrip(&mut g).unwrap();
}
