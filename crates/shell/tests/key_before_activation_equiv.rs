//! A key that reaches a client in one batch with `wl_keyboard.enter`, before the compositor's
//! reply to the `wl_display.sync` the client sends on that enter, compared with Quickshell on
//! Qt 6.12. Qt Quick sends a key only to the window's `activeFocusItem`, and the window is
//! activated (QtWayland's `handleWaylandSync`) in that reply:
//! - a window that was not active gets no `Keys.onPressed` / `Keys.onReleased` for the key;
//! - a window that was active, left and entered again in the same batch stays active (the
//!   leave and the enter cancel out) and gets the key.
//!
//! The batch is made without a clock: the client is stopped (SIGSTOP) while the click and the
//! key are sent, sway has answered a roundtrip after them, and then the client goes on.

use std::io::Write;
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use testenv::Log;
use wayland_client::protocol::{wl_output, wl_pointer, wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, delegate_noop};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1,
    zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};

const SCREEN: (u32, u32) = (1920, 1080);

/// A 400x100 panel in the top left corner that takes the keyboard when clicked; its item logs
/// its active focus and every key it gets.
const PANEL: &str = r#"import QtQuick
import Quickshell
import Quickshell.Wayland
ShellRoot {
    PanelWindow {
        anchors.top: true
        anchors.left: true
        implicitWidth: 400
        implicitHeight: 100
        exclusionMode: ExclusionMode.Ignore
        WlrLayershell.keyboardFocus: WlrKeyboardFocus.OnDemand
        Item {
            anchors.fill: parent
            focus: true
            onActiveFocusChanged: console.log("TXT af=" + activeFocus)
            Keys.onPressed: event => console.log("TXT p" + event.key)
            Keys.onReleased: event => console.log("TXT r" + event.key)
            Component.onCompleted: console.log("TXT ready")
        }
    }
}
"#;
/// A point of the panel, and one of the window sway tiles over the rest of the screen.
const ON_PANEL: (u32, u32) = (50, 50);
const ON_WINDOW: (u32, u32) = (900, 600);

/// Another client's window: sway gives it the keyboard when it maps.
const WINDOW: &str = r#"import QtQuick
import Quickshell
ShellRoot {
    FloatingWindow {
        implicitWidth: 300
        implicitHeight: 200
        Text { text: "other" }
    }
}
"#;

/// evdev key codes, and the Qt::Key of each.
const KEY_A: u32 = 30;
const KEY_B: u32 = 48;
const QT_A: u32 = 0x41;
const QT_B: u32 = 0x42;

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[derive(Default)]
struct Globals {
    seat: Option<wl_seat::WlSeat>,
    output: Option<wl_output::WlOutput>,
    vptr: Option<ZwlrVirtualPointerManagerV1>,
    vkbd: Option<ZwpVirtualKeyboardManagerV1>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for Globals {
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
                "wl_output" if g.output.is_none() => g.output = Some(reg.bind(name, 1, qh, ())),
                "zwlr_virtual_pointer_manager_v1" => {
                    g.vptr = Some(reg.bind(name, version.min(2), qh, ()))
                }
                "zwp_virtual_keyboard_manager_v1" => g.vkbd = Some(reg.bind(name, 1, qh, ())),
                _ => {}
            }
        }
    }
}

delegate_noop!(Globals: ignore wl_seat::WlSeat);
delegate_noop!(Globals: ignore wl_output::WlOutput);
delegate_noop!(Globals: ZwlrVirtualPointerManagerV1);
delegate_noop!(Globals: ZwlrVirtualPointerV1);
delegate_noop!(Globals: ZwpVirtualKeyboardManagerV1);
delegate_noop!(Globals: ZwpVirtualKeyboardV1);

/// The pointer and the keyboard of sway's seat (the headless seat has neither of its own).
struct Input {
    queue: EventQueue<Globals>,
    globals: Globals,
    ptr: ZwlrVirtualPointerV1,
    kbd: ZwpVirtualKeyboardV1,
    _keymap: std::fs::File,
    time: u32,
}

impl Input {
    fn connect(sway: &testenv::Sway) -> Option<Self> {
        let conn =
            Connection::from_socket(UnixStream::connect(sway.display()).expect("sway's socket"))
                .expect("a Wayland connection to sway");
        let mut queue = conn.new_event_queue();
        let qh = queue.handle();
        conn.display().get_registry(&qh, ());
        let mut globals = Globals::default();
        queue.roundtrip(&mut globals).unwrap();
        let (Some(seat), Some(vptr), Some(vkbd)) = (
            globals.seat.clone(),
            globals.vptr.clone(),
            globals.vkbd.clone(),
        ) else {
            testenv::unavailable(
                "compositor lacks zwlr_virtual_pointer_v1 or zwp_virtual_keyboard_v1",
            );
            return None;
        };
        let ptr = match &globals.output {
            Some(o) if wayland_client::Proxy::version(&vptr) >= 2 => {
                vptr.create_virtual_pointer_with_output(Some(&seat), Some(o), &qh, ())
            }
            _ => vptr.create_virtual_pointer(Some(&seat), &qh, ()),
        };
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
        .expect("xkb compiles the us keymap");
        let text = km.get_as_string(xkbcommon::xkb::KEYMAP_FORMAT_TEXT_V1);
        let mut keymap = tempfile::tempfile().unwrap();
        keymap.write_all(text.as_bytes()).unwrap();
        keymap.write_all(&[0]).unwrap();
        kbd.keymap(1, keymap.as_fd(), text.len() as u32 + 1);
        let mut p = Input {
            queue,
            globals,
            ptr,
            kbd,
            _keymap: keymap,
            time: 0,
        };
        p.sent();
        Some(p)
    }

    /// Sway has handled everything sent so far, and has queued the events that follow from it
    /// for its clients.
    fn sent(&mut self) {
        self.queue.roundtrip(&mut self.globals).unwrap();
        // Successive clicks are further apart than a double click.
        self.time += 500;
    }

    fn click(&mut self, (x, y): (u32, u32)) {
        const BTN_LEFT: u32 = 0x110;
        self.ptr
            .motion_absolute(self.time, x, y, SCREEN.0, SCREEN.1);
        self.ptr.frame();
        self.sent();
        for state in [
            wl_pointer::ButtonState::Pressed,
            wl_pointer::ButtonState::Released,
        ] {
            self.ptr.button(self.time, BTN_LEFT, state);
            self.ptr.frame();
            self.sent();
        }
    }

    /// Presses and releases the key `code`.
    fn key(&mut self, code: u32) {
        for state in [1, 0] {
            self.kbd.key(self.time, code, state);
            self.sent();
        }
    }
}

/// What a shell's QML logged after `TXT `, in order.
fn texts(log: &str) -> Vec<&str> {
    log.lines()
        .filter_map(|l| l.split_once("TXT ").map(|(_, t)| t.trim_end()))
        // Quickshell colours its lines.
        .map(|t| t.split('\u{1b}').next().unwrap())
        .collect()
}

/// A shell on a fixture under the test's sway, and its output as it arrives.
struct Client {
    name: &'static str,
    child: Kill,
    log: Arc<Log>,
}

impl Client {
    fn start(sway: &testenv::Sway, name: &'static str, dir: &Path, qml: &str) -> Self {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("shell.qml"), qml).unwrap();
        let (r, w) = std::io::pipe().unwrap();
        // libwayland writes its log to stderr in pieces: line-buffered, a line of it stays whole.
        let mut cmd = Command::new("stdbuf");
        cmd.arg("-eL");
        match name {
            "qs" => cmd.arg("qs"),
            _ => cmd.arg(env!("CARGO_BIN_EXE_impulse-shell")),
        };
        sway.env(&mut cmd)
            .arg("-p")
            .arg(dir)
            .envs(testenv::home_vars(&dir.join("home")))
            .env("RI_INTERP", "1")
            .env("QT_QPA_PLATFORM", "wayland")
            .env("WAYLAND_DEBUG", "1")
            .stdin(Stdio::null())
            .stdout(w.try_clone().unwrap())
            .stderr(w);
        let child = Kill(cmd.spawn().unwrap());
        drop(cmd);
        Client {
            name,
            child,
            log: Log::capture(r),
        }
    }

    /// Blocks until the QML logged `n` lines, which must be the first `n` of `all`.
    #[track_caller]
    fn logged(&self, what: &str, all: &[&str], n: usize) {
        if let Err(log) = self.log.wait(|l| texts(l).len() >= n) {
            panic!("{} exited before {what}:\n{log}", self.name);
        }
        let log = self.log.text();
        assert_eq!(texts(&log)[..n], all[..n], "{}: {what}\n{log}", self.name);
    }

    /// Stops the process: it reads nothing the compositor sends until [`Client::goes_on`].
    fn stop(&self) {
        let pid = self.child.0.id() as libc::pid_t;
        // SAFETY: kill(2) on the test's own child, which `Kill` has not reaped.
        assert_eq!(unsafe { libc::kill(pid, libc::SIGSTOP) }, 0);
        // SAFETY: siginfo_t is plain data, all zeroes is a value of it.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        // Returns once the child has stopped, and leaves it to `Kill` to reap.
        // SAFETY: waitid(2) on the test's own child; `info` is a siginfo_t for it to fill.
        let r = unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                &mut info,
                libc::WSTOPPED | libc::WNOWAIT,
            )
        };
        assert_eq!(r, 0, "waitid: {}", std::io::Error::last_os_error());
    }

    fn goes_on(&self) {
        let pid = self.child.0.id() as libc::pid_t;
        // SAFETY: kill(2) on the test's own child, which `Kill` has not reaped.
        assert_eq!(unsafe { libc::kill(pid, libc::SIGCONT) }, 0);
    }
}

/// The index of the first line from `from` on that holds every one of `parts`.
#[track_caller]
fn line_with(lines: &[&str], from: usize, parts: &[&str]) -> usize {
    lines
        .iter()
        .skip(from)
        .position(|l| parts.iter().all(|p| l.contains(p)))
        .map(|i| i + from)
        .unwrap_or_else(|| panic!("no line with {parts:?} from line {from} on:\n{lines:#?}"))
}

/// Runs the scenario on shell `name` and returns everything its QML logged. The panel has the
/// keyboard when it maps and loses it to the window; with `active` a click gives it back and
/// activates the panel. The panel's process is then stopped; a click on the window (with
/// `active`: the keyboard leaves the panel), a click on the panel (the keyboard enters it) and
/// the key A reach it in one batch when it goes on. The key B, pressed once the panel is
/// active, ends the scenario.
fn keys_got(name: &'static str, active: bool) -> Option<Vec<String>> {
    if !testenv::require_all(&["sway", "qs", "stdbuf"]) {
        return None;
    }
    let sway = testenv::Sway::start();
    let mut input = Input::connect(&sway)?;
    let dir = tempfile::tempdir().unwrap();
    let (p_a, r_a) = (format!("p{QT_A}"), format!("r{QT_A}"));
    let (p_b, r_b) = (format!("p{QT_B}"), format!("r{QT_B}"));
    let mut all = vec!["ready", "af=true", "af=false"];
    if active {
        all.extend(["af=true", &p_a, &r_a]);
    } else {
        // The key A came before the reply to `sync`: no item had active focus yet.
        all.push("af=true");
    }
    all.extend([p_b.as_str(), &r_b]);

    let panel = Client::start(&sway, name, &dir.path().join("panel"), PANEL);
    panel.logged("sway to give the mapped panel the keyboard", &all, 2);
    let _window = Client::start(&sway, "impulse-shell", &dir.path().join("window"), WINDOW);
    panel.logged("the mapped window to take the keyboard", &all, 3);
    if active {
        input.click(ON_PANEL);
        panel.logged("the clicked panel to be active", &all, 4);
    }

    panel.stop();
    if active {
        input.click(ON_WINDOW);
    }
    input.click(ON_PANEL);
    input.key(KEY_A);
    // Sway flushes a client's events when it has handled a batch of requests: after one more
    // answer the key's release is in the panel's socket too.
    input.sent();
    panel.goes_on();

    if !active {
        panel.logged("the reply to `sync` to activate the panel", &all, 4);
    }
    input.key(KEY_B);
    panel.logged("the key pressed in the active panel", &all, all.len());

    // The batch was what the comment says: the key came after the enter (with `active`, after
    // the leave and the enter) and before the panel answered anything.
    let log = panel.log.text();
    let lines: Vec<&str> = log.lines().collect();
    let key = format!(", {KEY_A}, 1)");
    let at = if active {
        let active = line_with(&lines, 0, &["TXT af=true"]);
        let active = line_with(&lines, active + 1, &["TXT af=true"]);
        let leave = line_with(&lines, active, &["wl_keyboard", ".leave("]);
        line_with(&lines, leave, &["wl_keyboard", ".enter("])
    } else {
        let inactive = line_with(&lines, 0, &["TXT af=false"]);
        line_with(&lines, inactive, &["wl_keyboard", ".enter("])
    };
    let key = line_with(&lines, at, &["wl_keyboard", ".key(", &key]);
    let next = line_with(&lines, at, &["TXT "]);
    assert!(
        key < next,
        "{name}: the key A reached the panel after it logged {:?}:\n{log}",
        lines[next]
    );
    assert_eq!(texts(&log), all, "{name}\n{log}");
    Some(all.into_iter().map(str::to_owned).collect())
}

#[test]
fn key_in_one_batch_with_enter_does_not_reach_a_window_that_was_not_active() {
    let Some(qs) = keys_got("qs", false) else {
        return;
    };
    assert_eq!(keys_got("impulse-shell", false), Some(qs));
}

#[test]
fn key_in_one_batch_with_leave_and_enter_reaches_a_window_that_stayed_active() {
    let Some(qs) = keys_got("qs", true) else {
        return;
    };
    assert_eq!(keys_got("impulse-shell", true), Some(qs));
}
