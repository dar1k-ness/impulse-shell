//! Paste reads the selection from the event loop (like `gdk_clipboard_read_text_async`): while
//! the selection owner holds its pipe open the shell keeps serving, nothing is inserted, and the
//! whole text goes in once the owner closes the pipe. The shell creates one wl_data_device.

use std::fs::File;
use std::io::Write;
use std::os::fd::AsFd;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use testenv::Log;
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, QueueHandle, delegate_noop, event_created_child};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1,
    zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
};
use wayland_protocols_wlr::data_control::v1::client::{
    zwlr_data_control_device_v1::{self, ZwlrDataControlDeviceV1},
    zwlr_data_control_manager_v1::ZwlrDataControlManagerV1,
    zwlr_data_control_offer_v1::ZwlrDataControlOfferV1,
    zwlr_data_control_source_v1::{self, ZwlrDataControlSourceV1},
};

const SHELL: &str = r#"import QtQuick
import QtQuick.Controls
import Rustshell
import Rustshell.Io
import Rustshell.Wayland
ShellRoot {
    PanelWindow {
        WlrLayershell.namespace: "paste"
        WlrLayershell.keyboardFocus: WlrKeyboardFocus.Exclusive
        anchors { top: true; left: true; right: true }
        implicitHeight: 100
        TextField {
            id: field
            focus: true
            onActiveFocusChanged: console.log("TXT focus " + activeFocus)
            onTextChanged: console.log("TXT text " + text)
        }
    }
    IpcHandler {
        target: "t"
        function text(): string {
            console.log("TXT asked");
            return "[" + field.text + "]";
        }
    }
}
"#;

/// The selection's text, written in two parts split inside the UTF-8 encoding of "é".
const FIRST: &[u8] = b"ab\xc3";
const REST: &[u8] = b"\xa9cd";

/// The selection owner: every reader gets FIRST at once and REST only once `finish` is set,
/// the pipe held open until then.
#[derive(Default)]
struct Owner {
    finish: bool,
    held: Vec<File>,
}

#[derive(Default)]
struct G {
    seat: Option<wl_seat::WlSeat>,
    vkbd: Option<ZwpVirtualKeyboardManagerV1>,
    data: Option<ZwlrDataControlManagerV1>,
    owner: Arc<Mutex<Owner>>,
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
                "zwlr_data_control_manager_v1" => g.data = Some(reg.bind(name, 1, qh, ())),
                _ => {}
            }
        }
    }
}

impl Dispatch<ZwlrDataControlSourceV1, ()> for G {
    fn event(
        g: &mut Self,
        _: &ZwlrDataControlSourceV1,
        ev: zwlr_data_control_source_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_data_control_source_v1::Event::Send { fd, .. } = ev {
            let mut f = File::from(fd);
            f.write_all(FIRST).unwrap();
            let mut o = g.owner.lock().unwrap();
            if o.finish {
                f.write_all(REST).unwrap();
            } else {
                o.held.push(f);
            }
        }
    }
}

impl Dispatch<ZwlrDataControlDeviceV1, ()> for G {
    fn event(
        _: &mut Self,
        _: &ZwlrDataControlDeviceV1,
        _: zwlr_data_control_device_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }

    event_created_child!(G, ZwlrDataControlDeviceV1, [
        zwlr_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ZwlrDataControlOfferV1, ()),
    ]);
}

delegate_noop!(G: ignore wl_seat::WlSeat);
delegate_noop!(G: ZwpVirtualKeyboardManagerV1);
delegate_noop!(G: ZwpVirtualKeyboardV1);
delegate_noop!(G: ZwlrDataControlManagerV1);
delegate_noop!(G: ignore ZwlrDataControlOfferV1);

struct Shell(Child);

impl Drop for Shell {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Before the fix a paste read the selection on the main loop with a 1 s deadline: the shell
/// froze meanwhile and then inserted the unfinished "ab".
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn paste_waits_for_the_owner_without_blocking_the_loop() {
    if !testenv::require("sway") {
        return;
    }
    let sway = testenv::Sway::start();
    let conn =
        Connection::from_socket(std::os::unix::net::UnixStream::connect(sway.display()).unwrap())
            .unwrap();
    let mut q = conn.new_event_queue();
    let qh = q.handle();
    conn.display().get_registry(&qh, ());
    let mut g = G::default();
    q.roundtrip(&mut g).unwrap();
    let (Some(seat), Some(vkbd), Some(data)) = (g.seat.clone(), g.vkbd.clone(), g.data.clone())
    else {
        testenv::unavailable("sway lacks zwp_virtual_keyboard_v1 or zwlr_data_control_v1");
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
    let dev = data.get_data_device(&seat, &qh, ());
    let src = data.create_data_source(&qh, ());
    src.offer("text/plain;charset=utf-8".into());
    src.offer("text/plain".into());
    dev.set_selection(Some(&src));
    q.roundtrip(&mut g).unwrap();
    let owner = g.owner.clone();
    // The owner answers readers from a thread of its own, as a separate client would.
    std::thread::spawn(move || while q.blocking_dispatch(&mut g).is_ok() {});

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("shell.qml"), SHELL).unwrap();
    let xdg = dir.path().join("xdg");
    let shell_env = |c: &mut Command| {
        sway.env(c)
            .arg("-p")
            .arg(dir.path())
            .env("RI_INTERP", "1")
            .env("XDG_CONFIG_HOME", xdg.join("config"))
            .env("XDG_STATE_HOME", xdg.join("state"))
            .env("XDG_CACHE_HOME", xdg.join("cache"))
            .stdin(Stdio::null());
    };
    let (r, out) = std::io::pipe().unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_impulse-shell"));
    shell_env(&mut cmd);
    let _sh = Shell(
        cmd.env("WAYLAND_DEBUG", "1")
            .stdout(out.try_clone().unwrap())
            .stderr(out)
            .spawn()
            .unwrap(),
    );
    let log = Log::capture(r);
    let call = |f: &str| {
        let mut c = Command::new(env!("CARGO_BIN_EXE_impulse-shell"));
        shell_env(&mut c);
        let o = c.args(["ipc", "call", "t", f]).output().unwrap();
        assert!(o.status.success(), "ipc call t {f}: {o:?}");
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    };

    log.wait(|t| t.contains("TXT focus true"))
        .unwrap_or_else(|t| panic!("the field never got the keyboard focus:\n{t}"));
    // Ctrl+V: evdev KEY_V with the us keymap's Control modifier.
    kbd.modifiers(4, 0, 0, 0);
    kbd.key(1, 47, 1);
    kbd.key(2, 47, 0);
    kbd.modifiers(0, 0, 0, 0);
    conn.flush().unwrap();
    log.wait(|t| {
        t.lines()
            .any(|l| l.contains("-> wl_data_offer") && l.contains(".receive("))
    })
    .unwrap_or_else(|t| panic!("Ctrl+V never read the clipboard:\n{t}"));
    // The owner holds every reader's pipe open: the loop that asked still answers, and the
    // field is unchanged.
    assert_eq!(call("text"), "[]");
    // The answer is no line of the log: the handler's own line is, and all the shell wrote
    // before the answer stands before it.
    log.wait(|t| t.contains("TXT asked"))
        .unwrap_or_else(|t| panic!("the handler's line never came:\n{t}"));
    assert!(!log.text().contains("TXT text a"));

    {
        let mut o = owner.lock().unwrap();
        o.finish = true;
        for mut f in o.held.drain(..) {
            f.write_all(REST).unwrap();
        }
    }
    log.wait(|t| t.contains("TXT text abécd"))
        .unwrap_or_else(|t| panic!("the pasted text never arrived:\n{t}"));
    assert_eq!(call("text"), "[abécd]");
    let t = log.text();
    let texts: Vec<&str> = t.lines().filter(|l| l.contains("TXT text ")).collect();
    assert_eq!(texts.len(), 1, "one insert of the whole text: {texts:#?}");
    let devices = t
        .lines()
        .filter(|l| l.contains("-> wl_data_device_manager") && l.contains(".get_data_device("))
        .count();
    assert_eq!(devices, 1, "one wl_data_device for the one seat");
}
