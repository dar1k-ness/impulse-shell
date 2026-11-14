//! `paste()` called from a script on a TextField and a TextEdit, on a real clipboard: the shell
//! under a sway of its own, the selection owned by this test and then by the shell itself.
//! QQuickTextInput::paste and QQuickTextControl::paste read QClipboard::text() inside the call.
//! The shell inserts inside the call when it has the clipboard's text in the process: its own
//! selection, or the text of the offer `wl_data_device.selection` gave it, which it reads as
//! soon as the offer comes. Until the owner has written that text a paste reads the owner's
//! pipe from the loop (tests/paste_async.rs) and the text comes after the script:
//! docs/qt-differences.md, which these tests hold in both directions. The signals and the
//! state a paste leaves are compared with Qt in keys.rs (script_paste_matches_qt).

use std::fs::File;
use std::io::Write;
use std::os::fd::AsFd;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};

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
    zwlr_data_control_offer_v1::{self, ZwlrDataControlOfferV1},
    zwlr_data_control_source_v1::{self, ZwlrDataControlSourceV1},
};

const SHELL: &str = r#"import QtQuick
import QtQuick.Controls
import Rustshell
import Rustshell.Io
import Rustshell.Wayland
ShellRoot {
    property string clip: Rustshell.clipboardText
    onClipChanged: console.log("TXT clip " + clip)
    PanelWindow {
        WlrLayershell.namespace: "paste"
        WlrLayershell.keyboardFocus: WlrKeyboardFocus.Exclusive
        anchors { top: true; left: true; right: true }
        implicitHeight: 100
        TextField {
            id: field
            focus: true
            onActiveFocusChanged: console.log("TXT focus " + activeFocus)
            onTextChanged: console.log("TXT field " + text)
        }
        TextEdit {
            id: edit
            y: 50
            onTextChanged: console.log("TXT edit " + text)
        }
        TextInput {
            id: fixed
            y: 80
            readOnly: true
            text: "fixed"
            onTextChanged: console.log("TXT fixed " + text)
        }
    }
    IpcHandler {
        target: "t"
        // Each returns what the script reads right after its paste() calls.
        function both(): string { field.paste(); fixed.paste(); edit.paste(); return text() }
        function own(): string { Rustshell.clipboardText = "mine"; edit.paste(); return text() }
        function copied(): string { field.selectAll(); field.copy(); edit.paste(); return text() }
        function again(): string { edit.paste(); return text() }
        function text(): string { return "[" + field.text + "|" + edit.text + "|" + fixed.text + "]" }
        function clip(): string { return Rustshell.clipboardText }
        function big(): string { Rustshell.clipboardText = "y".repeat(4194304); return "set" }
        function size(): string { return "" + edit.text.length }
    }
}
"#;

/// The test's sources: every reader of the source named `s` gets the text `s` + "é". The shell
/// reads each selection as soon as it is announced: once for `Rustshell.clipboardText`,
/// through the data-control device of its clipboard watcher, and with the keyboard focus once
/// more for `paste()`, through the offer of its wl_data_device.
#[derive(Default)]
struct G {
    seat: Option<wl_seat::WlSeat>,
    vkbd: Option<ZwpVirtualKeyboardManagerV1>,
    data: Option<ZwlrDataControlManagerV1>,
    /// The mime types of the offer the compositor is announcing.
    offer: Vec<String>,
    shell_owned: Owned,
    kept: Kept,
}

/// The readers the test's sources keep waiting: while `armed`, each reader's pipe is put in
/// `pipes` unanswered, as an owner that has not written its text yet leaves it. `answered`
/// counts the readers that got their text at once.
#[derive(Default)]
struct KeptReaders {
    armed: bool,
    pipes: Vec<File>,
    answered: usize,
}
type Kept = Arc<(Mutex<KeptReaders>, Condvar)>;

/// How many times a source of the shell (one without the test's `x-test/` type) became the
/// clipboard, as the test's own data-control device saw it.
type Owned = Arc<(Mutex<usize>, Condvar)>;

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

impl Dispatch<ZwlrDataControlSourceV1, &'static str> for G {
    fn event(
        g: &mut Self,
        _: &ZwlrDataControlSourceV1,
        ev: zwlr_data_control_source_v1::Event,
        name: &&'static str,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_data_control_source_v1::Event::Send { fd, .. } = ev {
            let mut pipe = File::from(fd);
            let mut kept = g.kept.0.lock().unwrap();
            if kept.armed {
                kept.pipes.push(pipe);
                g.kept.1.notify_all();
                return;
            }
            pipe.write_all(format!("{name}é").as_bytes()).unwrap();
            kept.answered += 1;
            g.kept.1.notify_all();
        }
    }
}

impl Dispatch<ZwlrDataControlDeviceV1, ()> for G {
    fn event(
        g: &mut Self,
        _: &ZwlrDataControlDeviceV1,
        ev: zwlr_data_control_device_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match ev {
            zwlr_data_control_device_v1::Event::DataOffer { .. } => g.offer.clear(),
            zwlr_data_control_device_v1::Event::Selection { id: Some(_) }
                if !g.offer.iter().any(|m| m.starts_with("x-test/")) =>
            {
                *g.shell_owned.0.lock().unwrap() += 1;
                g.shell_owned.1.notify_all();
            }
            _ => {}
        }
    }

    event_created_child!(G, ZwlrDataControlDeviceV1, [
        zwlr_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ZwlrDataControlOfferV1, ()),
    ]);
}

delegate_noop!(G: ignore wl_seat::WlSeat);
delegate_noop!(G: ZwpVirtualKeyboardManagerV1);
delegate_noop!(G: ZwpVirtualKeyboardV1);
delegate_noop!(G: ZwlrDataControlManagerV1);

impl Dispatch<ZwlrDataControlOfferV1, ()> for G {
    fn event(
        g: &mut Self,
        _: &ZwlrDataControlOfferV1,
        ev: zwlr_data_control_offer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_data_control_offer_v1::Event::Offer { mime_type } = ev {
            g.offer.push(mime_type);
        }
    }
}

struct Shell(Child);

impl Drop for Shell {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// How many times the shell has asked the clipboard offer the compositor gives the focused
/// client for its text: once when the offer comes, and once more for each paste that found
/// the text not read yet.
fn receives(log: &str) -> usize {
    log.lines()
        .filter(|l| l.contains("-> wl_data_offer") && l.contains(".receive("))
        .count()
}

/// How many times a source of the shell has been asked for its data.
fn sends(log: &str) -> usize {
    log.lines()
        .filter(|l| l.contains("data_control_source_v1") && l.contains(".send("))
        .count()
}

/// The end of a log that holds megabytes of pasted text.
fn tail(log: &str) -> String {
    log.lines()
        .rev()
        .take(40)
        .map(|l| l.chars().take(200).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The shell with the keyboard focus under a sway of its own, and this test as the other
/// client: the owner of the clipboard's first selection, "a".
struct Run {
    conn: Connection,
    qh: QueueHandle<G>,
    data: ZwlrDataControlManagerV1,
    dev: ZwlrDataControlDeviceV1,
    kbd: ZwpVirtualKeyboardV1,
    kept: Kept,
    shell_owned: Owned,
    log: std::sync::Arc<Log>,
    env: Box<dyn Fn(&mut Command)>,
    _shell: Shell,
    px: Option<testenv::HeldRequests>,
    _dir: tempfile::TempDir,
    _sway: testenv::Sway,
}

impl Run {
    /// `None` (reported) without a sway that has the protocols. `proxied`: the shell talks
    /// to the compositor through `px`.
    fn start(proxied: bool) -> Option<Run> {
        if !testenv::require("sway") {
            return None;
        }
        let sway = testenv::Sway::start();
        let socket = std::os::unix::net::UnixStream::connect(sway.display()).unwrap();
        let conn = Connection::from_socket(socket).unwrap();
        let mut q = conn.new_event_queue();
        let qh = q.handle();
        conn.display().get_registry(&qh, ());
        let mut g = G::default();
        q.roundtrip(&mut g).unwrap();
        let (Some(seat), Some(vkbd), Some(data)) = (g.seat.clone(), g.vkbd.clone(), g.data.clone())
        else {
            testenv::unavailable("sway lacks zwp_virtual_keyboard_v1 or zwlr_data_control_v1");
            return None;
        };
        // The headless seat has no keyboard until a (virtual) one with a keymap joins it; the
        // compositor gives the selection to the client with the keyboard focus.
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
        let (shell_owned, kept) = (g.shell_owned.clone(), g.kept.clone());

        let px = proxied.then(|| testenv::HeldRequests::start_at(sway.display().to_path_buf()));
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("shell.qml"), SHELL).unwrap();
        let env = {
            let (mut base, dir) = (Command::new("true"), dir.path().to_path_buf());
            sway.env(&mut base);
            let vars: Vec<_> = base
                .get_envs()
                .map(|(k, v)| (k.to_owned(), v.map(|v| v.to_owned())))
                .collect();
            let display = px.as_ref().map(|px| px.path().to_path_buf());
            Box::new(move |c: &mut Command| {
                for (k, v) in &vars {
                    match v {
                        Some(v) => c.env(k, v),
                        None => c.env_remove(k),
                    };
                }
                let xdg = dir.join("xdg");
                c.envs(display.as_ref().map(|d| ("WAYLAND_DISPLAY", d)))
                    .arg("-p")
                    .arg(&dir)
                    .env("RI_INTERP", "1")
                    .env("XDG_CONFIG_HOME", xdg.join("config"))
                    .env("XDG_STATE_HOME", xdg.join("state"))
                    .env("XDG_CACHE_HOME", xdg.join("cache"))
                    .stdin(Stdio::null());
            })
        };
        let run = |q: &mut wayland_client::EventQueue<G>, g: &mut G| {
            q.roundtrip(g).unwrap();
        };
        let src = data.create_data_source(&qh, "a");
        for m in ["text/plain;charset=utf-8", "text/plain", "x-test/a"] {
            src.offer(m.into());
        }
        dev.set_selection(Some(&src));
        run(&mut q, &mut g);
        // The owner answers readers from a thread of its own, as a separate client would.
        std::thread::spawn(move || while q.blocking_dispatch(&mut g).is_ok() {});

        let (r, out) = std::io::pipe().unwrap();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_rusty-impulse"));
        env(&mut cmd);
        let shell = Shell(
            cmd.env("WAYLAND_DEBUG", "1")
                .env("RI_LOG", "ri::keys=debug")
                .stdout(out.try_clone().unwrap())
                .stderr(out)
                .spawn()
                .unwrap(),
        );
        let run = Run {
            conn,
            qh,
            data,
            dev,
            kbd,
            kept,
            shell_owned,
            log: Log::capture(r),
            env,
            _shell: shell,
            px,
            _dir: dir,
            _sway: sway,
        };
        run.log
            .wait(|t| t.contains("TXT focus true"))
            .unwrap_or_else(|t| panic!("the field never got the keyboard focus:\n{t}"));
        // The shell has read the first selection's text twice: its watcher for
        // `Rustshell.clipboardText`, and its loop from the offer of the data device.
        run.wait("the watcher's text of the first selection", "TXT clip aé");
        run.wait("the first offer's text", "selection text read: 3 bytes");
        Some(run)
    }

    /// This test takes the clipboard with a source named `name`.
    fn own(&self, name: &'static str) {
        let src = self.data.create_data_source(&self.qh, name);
        for m in ["text/plain;charset=utf-8", "text/plain"] {
            src.offer(m.into());
        }
        src.offer(format!("x-test/{name}"));
        self.dev.set_selection(Some(&src));
        self.conn.flush().unwrap();
    }

    /// What the shell's script `f` returns: what it read right after its `paste()` calls.
    fn call(&self, f: &str) -> String {
        let mut c = Command::new(env!("CARGO_BIN_EXE_rusty-impulse"));
        (self.env)(&mut c);
        let o = c.args(["ipc", "call", "t", f]).output().unwrap();
        assert!(o.status.success(), "ipc call t {f}: {o:?}");
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }

    /// Ctrl+V: evdev KEY_V with the us keymap's Control modifier.
    fn press_paste(&self) {
        self.kbd.modifiers(4, 0, 0, 0);
        self.kbd.key(1, 47, 1);
        self.kbd.key(2, 47, 0);
        self.kbd.modifiers(0, 0, 0, 0);
        self.conn.flush().unwrap();
    }

    /// `wl-paste` of the test's sway whose output nobody reads yet: it fills that pipe and
    /// stops reading the owner's.
    fn stalled_reader(&self) -> Child {
        let mut c = Command::new("wl-paste");
        self._sway.env(&mut c);
        c.arg("--no-newline")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap()
    }

    /// Waits until the shell's sources have been asked for their data `n` times.
    fn sends(&self, n: usize) {
        self.log
            .wait(|t| sends(t) >= n)
            .unwrap_or_else(|t| panic!("no {n} reads of the shell's sources:\n{}", tail(&t)));
    }

    /// The call's answer comes over the IPC socket, the shell's log over a pipe: what the
    /// shell printed is waited for before the log is read.
    fn wait(&self, what: &str, line: &str) {
        self.log
            .wait(|t| t.lines().any(|l| l.ends_with(line)))
            .unwrap_or_else(|t| panic!("{what}: no line {line:?}:\n{t}"));
    }

    /// Waits until the shell's data device has the offer of the test's source `name`.
    fn offered(&self, name: &str) {
        let mime = format!("x-test/{name}");
        self.log
            .wait(|t| {
                t.find(&mime).is_some_and(|at| {
                    t[at..]
                        .lines()
                        .any(|l| l.contains("wl_data_device") && l.contains(".selection("))
                })
            })
            .unwrap_or_else(|t| panic!("the shell never got the offer of {name}:\n{t}"));
    }
}

/// Before the fix `paste()` was no method of the engine's editors: the call did nothing.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn script_paste_inserts_in_the_call_with_the_text_and_after_the_script_without() {
    let Some(run) = Run::start(false) else { return };
    // Another client owns the clipboard and the shell has read the offer's text. Three calls
    // in one script, which reads the editors after them: the field has the text, the
    // read-only input nothing, the TextEdit, which has no focus, the text too. The paste
    // does not read the offer again.
    assert_eq!(receives(&run.log.text()), 1);
    assert_eq!(run.call("both"), "[aé|aé|fixed]");
    run.wait("paste of the offer's text", "TXT edit aé");
    let t = run.log.text();
    assert!(
        t.find("TXT field aé") < t.find("TXT edit aé"),
        "the pastes insert in call order"
    );
    assert_eq!(receives(&t), 1);

    // The shell owns the clipboard: its own text inside the call (QWaylandClipboard::mimeData),
    // also before the compositor has announced the selection.
    assert_eq!(run.call("own"), "[aé|aémine|fixed]");
    assert_eq!(run.call("copied"), "[aé|aémineaé|fixed]");
    run.wait("paste of the shell's own text", "TXT edit aémineaé");

    // The compositor has to have both of the shell's sources before another client takes the
    // clipboard back, or the shell's second one would take it from that client.
    let n = run.shell_owned.0.lock().unwrap();
    drop(run.shell_owned.1.wait_while(n, |n| *n < 2).unwrap());
    // Another client takes the clipboard and answers no reader yet. The shell drops both of
    // its sources once the compositor has cancelled them, gets the new offer and asks for
    // its text; so does its watcher.
    run.kept.0.lock().unwrap().armed = true;
    run.own("bbb");
    run.log
        .wait(|t| {
            t.lines()
                .filter(|l| {
                    l.contains("-> ")
                        && l.contains("data_control_source_v1")
                        && l.contains(".destroy(")
                })
                .count()
                == 2
        })
        .unwrap_or_else(|t| panic!("the shell never dropped its sources:\n{t}"));
    run.offered("bbb");
    let kept = |n: usize| {
        let k = run.kept.0.lock().unwrap();
        drop(run.kept.1.wait_while(k, |k| k.pipes.len() < n).unwrap());
    };
    kept(2);
    // The shell's log comes over a pipe, after the owner has heard of the read.
    let asked = 2;
    run.log
        .wait(|t| receives(t) == asked)
        .unwrap_or_else(|t| panic!("the shell never asked the new offer for its text:\n{t}"));
    // The difference from Qt: the new selection's text is not in the process, so the script
    // reads the old text after `paste()`, and the text comes once the owner has answered the
    // read the paste started. Qt would wait for the owner inside the call.
    assert_eq!(run.call("again"), "[aé|aémineaé|fixed]");
    kept(3);
    assert_eq!(run.call("text"), "[aé|aémineaé|fixed]");
    // The owner answers every reader: the paste inserts, and from then on the text is in
    // the process again.
    {
        let mut k = run.kept.0.lock().unwrap();
        k.armed = false;
        for mut pipe in k.pipes.drain(..) {
            pipe.write_all("bbbé".as_bytes()).unwrap();
        }
    }
    run.wait("paste from the new owner", "TXT edit aémineaébbbé");
    run.wait("the new offer's text", "selection text read: 5 bytes");
    assert_eq!(run.call("again"), "[aé|aémineaébbbébbbé|fixed]");
    run.wait("paste of the new offer's text", "TXT edit aémineaébbbébbbé");
    let t = run.log.text();
    assert_eq!(
        receives(&t),
        asked + 1,
        "one read for the paste that waited"
    );
    for (name, n) in [("field", 1), ("edit", 5), ("fixed", 0)] {
        let lines: Vec<&str> = t
            .lines()
            .filter(|l| l.contains(&format!("TXT {name} ")))
            .collect();
        assert_eq!(lines.len(), n, "one insert for each paste: {lines:#?}");
    }
}

/// The text `paste()` inserts inside the call is the text of the offer the shell's own
/// wl_data_device holds, on the connection its scripts are driven from: the selection, its
/// text and the call come in that connection's order. The clipboard watcher has a connection
/// of its own, which here hears of the new selection only after the paste. Before the fix
/// `paste()` took the watcher's text: the text of the selection before.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn script_paste_inserts_the_text_of_the_data_device_s_offer() {
    let Some(run) = Run::start(true) else { return };
    let px = run.px.as_ref().unwrap();
    assert_eq!(run.call("again"), "[|aé|fixed]");
    run.wait("paste of the first offer's text", "TXT edit aé");
    // From here the compositor's events reach every connection of the shell but the one
    // that uses data-control and draws nothing: the watcher's.
    px.hold_events_of("data_control_manager_v1", "wl_compositor");
    run.own("bbb");
    run.offered("bbb");
    run.wait("the new offer's text", "selection text read: 5 bytes");
    let asked = receives(&run.log.text());
    assert_eq!(run.call("again"), "[|aébbbé|fixed]");
    run.wait("paste of the new offer's text", "TXT edit aébbbé");
    // The watcher has not heard of the selection: the first one's readers were the watcher
    // and the data device, the second one's the data device alone.
    assert_eq!(run.call("clip"), "aé");
    assert_eq!(run.kept.0.lock().unwrap().answered, 3);
    assert_eq!(receives(&run.log.text()), asked);
    px.release_events();
    run.wait("the watcher's text of the new selection", "TXT clip bbbé");
    assert_eq!(run.kept.0.lock().unwrap().answered, 4);
}

/// The shell's own text is the clipboard's until the compositor cancels the shell's source,
/// and the source is on the connection of the data device: its `cancelled` comes before the
/// offer of the selection that replaced it, so `paste()` never inserts the shell's text once
/// the shell has that offer. Before the fix the source had a connection of its own, which
/// here hears of nothing after the shell's copy: `paste()` inserted the shell's old text.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn script_paste_inserts_the_text_of_the_selection_that_replaced_the_shells() {
    let Some(run) = Run::start(true) else { return };
    let px = run.px.as_ref().unwrap();
    assert_eq!(run.call("own"), "[|mine|fixed]");
    // The compositor has the shell's source before another client takes the clipboard.
    let n = run.shell_owned.0.lock().unwrap();
    drop(run.shell_owned.1.wait_while(n, |n| *n < 1).unwrap());
    px.hold_events_of("data_control_manager_v1", "wl_compositor");
    run.own("bbb");
    run.offered("bbb");
    run.wait("the new offer's text", "selection text read: 5 bytes");
    assert_eq!(run.call("again"), "[|minebbbé|fixed]");
    run.wait("paste of the new offer's text", "TXT edit minebbbé");
    px.release_events();
    run.wait("the watcher's text of the new selection", "TXT clip bbbé");
}

/// The shell's own selection is pasted from the source itself, by a script and by the paste
/// key: nothing reads the offer the compositor sends the shell for it, and the source gets
/// no `send` for the paste. Before the source was on the shell's connection the paste key
/// read the shell's own offer through a pipe.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn the_shells_own_text_is_pasted_without_a_read() {
    let Some(run) = Run::start(false) else { return };
    assert_eq!(run.call("own"), "[|mine|fixed]");
    // The compositor has the shell's source, the shell its offer, and the watcher its text:
    // the one read of the source.
    run.wait(
        "the watcher's text of the shell's selection",
        "TXT clip mine",
    );
    run.log
        .wait(|t| {
            t.lines()
                .filter(|l| l.contains("wl_data_device") && l.contains(".selection("))
                .count()
                == 2
        })
        .unwrap_or_else(|t| panic!("the shell never got the offer of its own source:\n{t}"));
    run.sends(1);
    let asked = || {
        let t = run.log.text();
        (receives(&t), sends(&t))
    };
    let before = asked();
    assert_eq!(run.call("again"), "[|minemine|fixed]");
    run.press_paste();
    run.wait("the paste key's text", "TXT field mine");
    // A call after the key: everything the paste asked for is in the log.
    assert_eq!(run.call("text"), "[mine|minemine|fixed]");
    assert_eq!(asked(), before, "a paste read the shell's own offer");

    // Another client's selection after it: the source's `cancelled` comes before the offer,
    // so the offer is read when it comes, and once more by the paste key, which always asks
    // the owner.
    run.own("bbb");
    run.offered("bbb");
    run.wait("the new offer's text", "selection text read: 5 bytes");
    run.press_paste();
    run.wait("the paste key's text", "TXT field minebbbé");
    assert_eq!(asked(), (before.0 + 2, before.1));
}

/// The shell answers the readers of its selection from its loop: a reader that stops reading
/// leaves a pipe the loop writes when it can be written, and the loop goes on meanwhile; the
/// compositor cancelling the source ends the read.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_reader_that_stops_reading_does_not_hold_the_shells_loop() {
    const BIG: usize = 4 << 20;
    let Some(run) = Run::start(false) else { return };
    assert_eq!(run.call("big"), "set");
    // The watcher's read of it.
    run.sends(1);
    let mut stalled = run.stalled_reader();
    run.sends(2);
    // The reader has what two pipes hold and reads no more; the loop answers a call, and
    // pastes the text it owns.
    assert_eq!(run.call("text"), "[||fixed]");
    assert_eq!(run.call("again").len(), "[||fixed]".len() + BIG);
    assert_eq!(run.call("size"), BIG.to_string());
    // The reader reads on: the loop writes the rest.
    let mut got = Vec::new();
    std::io::Read::read_to_end(&mut stalled.stdout.take().unwrap(), &mut got).unwrap();
    assert!(stalled.wait().unwrap().success());
    assert!(got.len() == BIG && got.iter().all(|b| *b == b'y'));

    // A second reader stops the same way, and the shell's next selection replaces the
    // source: the read ends with what the reader had taken.
    let mut stalled = run.stalled_reader();
    run.sends(3);
    assert_eq!(run.call("own").len(), "[||fixed]".len() + BIG + 4);
    let mut got = Vec::new();
    std::io::Read::read_to_end(&mut stalled.stdout.take().unwrap(), &mut got).unwrap();
    let _ = stalled.wait();
    assert!(
        got.len() < BIG && got.iter().all(|b| *b == b'y'),
        "the replaced source went on serving: {} bytes",
        got.len()
    );
    assert_eq!(run.call("size"), (BIG + 4).to_string());
}
