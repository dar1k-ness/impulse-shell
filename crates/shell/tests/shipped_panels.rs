//! The panels of the shell as it runs on the PC: `impulse-shell -p <QML tree>` on the tree its code was
//! compiled from, every script as compiled code, under a sway of its own. Each test opens one
//! panel the way a user does (its IPC target, a notification, a config value, the settings
//! window's own process), and fails unless the panel comes on screen with a picture in it,
//! reacts to a click and to a key, and the shell logs no script error meanwhile.
//! `shipped_first_frame` proves only the bar's first frame; a panel whose scripts fail opens
//! empty or not at all while the bar keeps drawing.
//!
//! Every wait is on an event of the shell's own log (`WAYLAND_DEBUG=1` and `RI_LOG` on one
//! stderr): a request it sent, an event it got, a frame presented. A panel that never opens, or
//! never reacts, leaves its test waiting, and nextest's slow-timeout ends it; the last
//! `waiting for` line in the test's output names what never came.
//!
//! Two writers share that stderr, and neither writes a line at once: libwayland's stdio writes
//! an unbuffered line 128 bytes at a time, and the shell's own lines go out one format piece at
//! a time, so a line of one lands inside a line of the other. impulse-shell runs under `stdbuf -eL`,
//! which makes each libwayland line one write, and [`untangled`] takes those lines out of the
//! shell's own.
//!
//! RI_SHIPPED_BIN and RI_SHIPPED_TREE name another binary and tree to run (the gate's installed
//! ones). The original reads the compositor's state from Hyprland's sockets, so the tests answer on them
//! for a Hyprland with one monitor, the sway output, and one empty workspace: without it the
//! session screen and the overview size themselves from a monitor that does not exist.

#[path = "../../qml/tests/hypr_ipc/mod.rs"]
mod hypr_ipc;

use std::collections::{BTreeSet, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::AsFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};

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

/// The sway output (`testenv::Sway`'s config).
const SCREEN: (u32, u32) = (1920, 1080);
/// A panel that draws has at least this many screen pixels of its own, in at least
/// [`MIN_COLOURS`] colours: the smallest one here, a notification popup, covers 24000, and
/// antialiased text alone brings dozens of colours, while a panel that failed to fill itself
/// is one flat rectangle or nothing.
const MIN_PIXELS: usize = 5000;
const MIN_COLOURS: usize = 16;
/// `qml: ` lines that are not script errors: cliphist has no database in a fresh home, and the
/// clipboard port reports its exit code.
const NOT_SCRIPT_ERRORS: &[&str] = &["qml: ClipboardHistory: listing failed with code 1"];
/// Script errors the shell does log on this tree: real faults, found by these tests and not
/// fixed with them. Each is the start of its line and what follows the item's number. A test
/// prints the ones it meets and fails once one is gone, so that the fix takes its entry off.
///
/// There is none now: the binding loop of `NavigationRailTabArray.collapsedWidth` (#86) went
/// with the text that measures again after a pending binding (`World::measure_text`).
const KNOWN_FAULTS: Faults = &[];
type Faults = &'static [(&'static str, &'static str)];

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
    vptr: Option<ZwlrVirtualPointerManagerV1>,
    vkbd: Option<ZwpVirtualKeyboardManagerV1>,
    output: Option<wl_output::WlOutput>,
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
                "zwlr_virtual_pointer_manager_v1" => {
                    g.vptr = Some(reg.bind(name, version.min(2), qh, ()))
                }
                "zwp_virtual_keyboard_manager_v1" => g.vkbd = Some(reg.bind(name, 1, qh, ())),
                "wl_output" if g.output.is_none() => g.output = Some(reg.bind(name, 1, qh, ())),
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

/// The seat's only pointer and keyboard: headless sway has none of its own.
struct Input {
    queue: EventQueue<Globals>,
    globals: Globals,
    ptr: ZwlrVirtualPointerV1,
    kbd: ZwpVirtualKeyboardV1,
    _keymap: std::fs::File,
    time: u32,
}

/// evdev key codes.
const KEY_ESC: u32 = 1;
const KEY_A: u32 = 30;
const KEY_RIGHT: u32 = 106;
const KEY_F: u32 = 33;
const KEY_END: u32 = 107;

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

    /// Sway has handled everything sent so far.
    fn sent(&mut self) {
        self.queue.roundtrip(&mut self.globals).unwrap();
        // Successive clicks are further apart than a double click.
        self.time += 500;
    }

    fn move_to(&mut self, (x, y): (i32, i32)) {
        self.ptr
            .motion_absolute(self.time, x as u32, y as u32, SCREEN.0, SCREEN.1);
        self.ptr.frame();
        self.sent();
    }

    fn button(&mut self, state: wl_pointer::ButtonState) {
        const BTN_LEFT: u32 = 0x110;
        self.ptr.button(self.time, BTN_LEFT, state);
        self.ptr.frame();
        self.sent();
    }

    /// Presses and releases the key `code`, with Control held when `ctrl`.
    fn key(&mut self, code: u32, ctrl: bool) {
        const KEY_LEFTCTRL: u32 = 29;
        // xkb's Control modifier in the us keymap.
        const CONTROL: u32 = 4;
        if ctrl {
            self.kbd.key(self.time, KEY_LEFTCTRL, 1);
            self.kbd.modifiers(CONTROL, 0, 0, 0);
        }
        self.kbd.key(self.time, code, 1);
        self.sent();
        self.kbd.key(self.time, code, 0);
        if ctrl {
            self.kbd.key(self.time, KEY_LEFTCTRL, 0);
            self.kbd.modifiers(0, 0, 0, 0);
        }
        self.sent();
    }
}

/// Hyprland's two sockets, as much of them as the original asks at startup and when a panel opens.
struct Hypr {
    requests: Arc<(Mutex<Vec<String>>, Condvar)>,
}

impl Hypr {
    const SIGNATURE: &str = "gate";

    fn answer(command: &str) -> String {
        let monitor = format!(
            r#"{{"id":0,"name":"HEADLESS-1","description":"headless","make":"","model":"","serial":"","width":{},"height":{},"refreshRate":60.0,"x":0,"y":0,"activeWorkspace":{{"id":1,"name":"1"}},"specialWorkspace":{{"id":0,"name":""}},"reserved":[0,0,0,0],"scale":1.0,"transform":0,"focused":true,"dpmsStatus":true,"vrr":false,"disabled":false,"availableModes":["1920x1080@60.00Hz"]}}"#,
            SCREEN.0, SCREEN.1
        );
        let workspace = r#"{"id":1,"name":"1","monitor":"HEADLESS-1","monitorID":0,"windows":0,"hasfullscreen":false,"lastwindow":"0x0","lastwindowtitle":"","ispersistent":false}"#;
        match command.trim().trim_start_matches("j/") {
            "monitors" | "monitors all" => format!("[{monitor}]"),
            "workspaces" => format!("[{workspace}]"),
            "activeworkspace" => workspace.into(),
            "clients" | "binds" => "[]".into(),
            "layers" | "activewindow" => "{}".into(),
            "devices" => r#"{"mice":[],"keyboards":[{"address":"0x1","name":"keyboard","rules":"","model":"","layout":"us","variant":"","options":"","active_keymap":"English (US)","main":true}],"tablets":[],"touch":[],"switches":[]}"#.into(),
            _ => "ok".into(),
        }
    }

    fn listen(runtime_dir: &Path) -> Self {
        let dir = runtime_dir.join("hypr").join(Self::SIGNATURE);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("hyprland.lock"), "1\nwayland-1\n").unwrap();
        let events = UnixListener::bind(dir.join(".socket2.sock")).unwrap();
        // The event socket stays silent: nothing happens in this Hyprland.
        std::thread::spawn(move || {
            let mut held = Vec::new();
            for c in events.incoming() {
                held.push(c);
            }
        });
        let socket = UnixListener::bind(dir.join(".socket.sock")).unwrap();
        let requests = Arc::new((Mutex::new(Vec::new()), Condvar::new()));
        let seen = requests.clone();
        std::thread::spawn(move || {
            for mut c in socket.incoming().flatten() {
                // hyprctl's protocol: one write is the request, the reply ends at the close.
                let mut buf = [0; 65536];
                let n = c.read(&mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]).into_owned();
                let _ = c.write_all(hypr_ipc::answer(&request, Self::answer).as_bytes());
                seen.0.lock().unwrap().push(request);
                seen.1.notify_all();
            }
        });
        Hypr { requests }
    }

    /// Blocks until the original sent a request that `is` matches.
    fn wait(&self, what: &str, is: impl Fn(&str) -> bool) {
        eprintln!("waiting for {what}");
        let seen = self.requests.0.lock().unwrap();
        let _seen = self
            .requests
            .1
            .wait_while(seen, |seen| !seen.iter().any(|r| is(r)))
            .unwrap();
    }
}

/// The screen as grim sees it.
struct Shot(Vec<[u8; 3]>);

/// A screen rectangle: left, top, right, bottom.
type Rect = (usize, usize, usize, usize);

impl Shot {
    /// How many pixels inside `rect` differ from `before`, and how many colours they have now.
    fn changed(&self, before: &Shot, (x0, y0, x1, y1): Rect) -> (usize, usize) {
        let mut colours = HashSet::new();
        let mut n = 0;
        for y in y0..y1 {
            let row = y * SCREEN.0 as usize;
            for i in row + x0..row + x1 {
                if self.0[i] != before.0[i] {
                    n += 1;
                    colours.insert(self.0[i]);
                }
            }
        }
        (n, colours.len())
    }
}

/// A surface of the shell: its `wl_surface` id and the role object whose `destroy` closes it.
#[derive(Clone, Debug)]
struct Surface {
    name: String,
    wl: String,
    role: String,
}

/// Where a WAYLAND_DEBUG line starts in `line`: its `[hh:mm:ss.uuuuuu] ` stamp.
fn wayland_line(line: &str) -> Option<usize> {
    const STAMP: &[u8] = b"[dd:dd:dd.dddddd] ";
    let b = line.as_bytes();
    (0..b.len().saturating_sub(STAMP.len() - 1)).find(|&i| {
        STAMP.iter().zip(&b[i..]).all(|(s, c)| match s {
            b'd' => c.is_ascii_digit(),
            _ => s == c,
        })
    })
}

/// The log with every line whole. libwayland writes a line at once (`stdbuf -eL`), the shell
/// writes its own in pieces, so a libwayland line can land inside one of the shell's: it is
/// taken out, and the pieces around it are joined. Each line of `log` gives one line here, so
/// line numbers hold while the log grows.
fn untangled(log: &str) -> String {
    let mut out = String::with_capacity(log.len());
    let mut piece = String::new();
    for line in log.lines() {
        match wayland_line(line) {
            Some(at) => {
                piece.push_str(&line[..at]);
                out.push_str(&line[at..]);
            }
            None => {
                out.push_str(&piece);
                out.push_str(line);
                piece.clear();
            }
        }
        out.push('\n');
    }
    out
}

/// The digits `line` has right after `key`.
fn id_after<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let (_, r) = line.split_once(key)?;
    let end = r.find(|c: char| !c.is_ascii_digit()).unwrap_or(r.len());
    (end > 0).then(|| &r[..end])
}

/// The last layer surface created for `namespace`.
fn layer_surface(lines: &[&str], namespace: &str) -> Option<Surface> {
    let ns = format!("\"{namespace}\")");
    lines
        .iter()
        .rev()
        .filter(|l| l.contains(".get_layer_surface(") && l.contains(&ns))
        .find_map(|l| {
            Some(Surface {
                name: namespace.into(),
                wl: id_after(l, "wl_surface#")?.into(),
                role: format!(
                    "zwlr_layer_surface_v1#{}",
                    id_after(l, "new id zwlr_layer_surface_v1#")?
                ),
            })
        })
}

/// The last surface created by a request `request(new id <interface>#N, wl_surface#M`: a
/// session lock surface, or the xdg_surface of a toplevel.
fn role_surface(lines: &[&str], name: &str, request: &str, interface: &str) -> Option<Surface> {
    let new = format!("new id {interface}#");
    lines
        .iter()
        .rev()
        .filter(|l| l.contains(request))
        .find_map(|l| {
            Some(Surface {
                name: name.into(),
                wl: id_after(l, "wl_surface#")?.into(),
                role: format!("{interface}#{}", id_after(l, &new)?),
            })
        })
}

/// How many frames of `surface` came on screen: the wp_presentation_feedback objects requested
/// for it that came back `presented`.
fn presented(lines: &[&str], surface: &Surface) -> usize {
    let on = format!(
        ".feedback(wl_surface#{}, new id wp_presentation_feedback#",
        surface.wl
    );
    let mut pending = HashSet::new();
    let mut n = 0;
    for l in lines {
        if let Some(id) = id_after(l, &on) {
            pending.insert(id);
        } else if let Some(id) = id_after(l, "wp_presentation_feedback#")
            && (l.contains(".presented(") || l.contains(".discarded("))
            && pending.remove(id)
            && l.contains(".presented(")
        {
            n += 1;
        }
    }
    n
}

/// impulse-shell closed `surface` and sway has taken it off the screen: sway answers the request
/// that destroys an object with `delete_id` once it has carried it out.
fn destroyed(lines: &[&str], surface: &Surface) -> bool {
    let gone = format!("-> {}.destroy()", surface.role);
    let id = surface.role.rsplit('#').next().unwrap();
    let deleted = format!("wl_display#1.delete_id({id})");
    lines
        .iter()
        .skip_while(|l| !l.contains(&gone))
        .any(|l| l.ends_with(&deleted))
}

/// Where the pointer is on `surface` after the last pointer event, in the surface's own
/// coordinates; None while it is on another surface.
fn pointer_on(lines: &[&str], surface: &Surface) -> Option<(f64, f64)> {
    let mut at = None;
    let pair = |r: &str| {
        let mut n = r.trim_end_matches(')').rsplit(", ");
        let y = n.next()?.parse().ok()?;
        Some((n.next()?.parse().ok()?, y))
    };
    for l in lines {
        let Some((_, r)) = l.split_once("] wl_pointer#") else {
            continue;
        };
        if r.contains(".enter(") {
            at = (id_after(r, "wl_surface#") == Some(&surface.wl))
                .then(|| pair(r))
                .flatten();
        } else if r.contains(".leave(") {
            at = None;
        } else if r.contains(".motion(") && at.is_some() {
            at = pair(r);
        }
    }
    at
}

/// A `qml: ` line is how main.rs `log_errors` reports an error a script raised; a panic is
/// the same fault found later.
fn script_errors(faults: Faults, log: &str) -> Vec<&str> {
    log.lines()
        .filter(|l| {
            l.starts_with("qml: ") || l.contains("panicked") || l.contains("no compiled code")
        })
        .filter(|l| !NOT_SCRIPT_ERRORS.contains(l) && known_fault(faults, l).is_none())
        .collect()
}

/// The entry of `faults` that `line` is.
fn known_fault(faults: Faults, line: &str) -> Option<usize> {
    faults.iter().position(|(start, rest)| {
        line.strip_prefix(start).is_some_and(|l| {
            l.trim_start_matches(|c: char| c.is_ascii_digit())
                .starts_with(rest)
        })
    })
}

/// The `faults` that `log` holds, each line once, and the entries it does not hold.
fn known_faults(faults: Faults, log: &str) -> (BTreeSet<&str>, Vec<usize>) {
    let met: BTreeSet<&str> = log
        .lines()
        .filter(|l| known_fault(faults, l).is_some())
        .collect();
    let gone = (0..faults.len())
        .filter(|i| !met.iter().any(|l| known_fault(faults, l) == Some(*i)))
        .collect();
    (met, gone)
}

fn tail(log: &str) -> String {
    let lines: Vec<&str> = log
        .lines()
        .filter(|l| !l.contains(" -> ") && !l.contains("@") && !l.starts_with('['))
        .collect();
    lines[lines.len().saturating_sub(40)..].join("\n")
}

/// Something a pointer can press, as `ipc call ri-debug dump` lists it (main.rs
/// `dump_clickables`): its rectangle in its window, its type chain and its label.
#[derive(Debug)]
struct Clickable {
    rect: [f64; 4],
    chain: String,
    label: String,
}

/// One impulse-shell on the shipped tree, with a home, a sway, a pointer and a Hyprland of its own.
/// Fields drop in this order: impulse-shell before the sway it draws on.
struct Shell {
    log: Arc<Log>,
    _child: Kill,
    input: Input,
    /// The screen before impulse-shell started.
    empty: Shot,
    hypr: Hypr,
    path: PathBuf,
    bin: PathBuf,
    bus: Option<Bus>,
    home: tempfile::TempDir,
    sway: testenv::Sway,
}

/// A session bus with no service activation (a real one starts whatever is installed, and a
/// keyring prompt takes the keyboard).
struct Bus {
    address: String,
    _daemon: Kill,
}

impl Bus {
    fn start(dir: &Path) -> Self {
        let address = format!("unix:path={}", dir.join("bus").display());
        let conf = dir.join("bus.conf");
        std::fs::write(
            &conf,
            format!(
                r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN" "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>{address}</listen>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#
            ),
        )
        .unwrap();
        let mut daemon = Command::new("dbus-daemon")
            .arg("--config-file")
            .arg(&conf)
            .args(["--nofork", "--print-address"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()
            .expect("dbus-daemon");
        // It prints its address once it listens.
        let mut line = String::new();
        BufReader::new(daemon.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert!(line.starts_with("unix:"), "dbus-daemon printed {line:?}");
        Bus {
            address,
            _daemon: Kill(daemon),
        }
    }
}

/// The heights impulse-shell gave `surface`'s input region, in order: a panel that grows or shrinks
/// sets a new one.
fn input_heights(lines: &[&str], surface: &Surface) -> Vec<i64> {
    let set = format!("-> wl_surface#{}.set_input_region(wl_region#", surface.wl);
    let mut added: Vec<(&str, i64)> = Vec::new();
    let mut heights = Vec::new();
    for l in lines {
        if let Some(id) = id_after(l, &set) {
            heights.push(added.iter().rev().find(|a| a.0 == id).map_or(0, |a| a.1));
        } else if let Some(id) = id_after(l, "-> wl_region#")
            && let Some((_, r)) = l.split_once(".add(")
            && let Some(h) = r.trim_end_matches(')').rsplit(", ").next()
        {
            added.push((id, h.parse().unwrap_or(0)));
        }
    }
    heights
}

/// `surface`, a layer, asks the compositor for no keyboard: there is no key to press in it.
fn takes_no_keyboard(lines: &[&str], surface: &Surface) -> bool {
    let asks = format!("-> {}.set_keyboard_interactivity(", surface.role);
    let mut asked = lines
        .iter()
        .filter_map(|l| l.split_once(&asks))
        .map(|a| a.1);
    asked.clone().count() > 0 && asked.all(|a| a == "0)")
}

/// The shell gave keyboard focus to an item (keys.rs logs each change of it).
fn focus_given(lines: &[&str]) -> bool {
    lines
        .iter()
        .any(|l| l.starts_with("focus win=Some(") && l.contains(" -> Some("))
}

/// The keyboard came back to an item of type `ty` from the item it had left it for (keys.rs
/// logs each change of it with the type of the item that gets it).
fn focus_back(lines: &[&str], ty: &str) -> bool {
    let moves: Vec<(&str, &str, &str)> = (lines.iter())
        .filter_map(|l| {
            let (from, to) =
                (l.strip_prefix("focus win=")?.split_once(' ')?.1).split_once(" -> ")?;
            let (to, ty) = to.split_once(' ')?;
            Some((from, to, ty))
        })
        .collect();
    let ty = format!("Some(\"{ty}\")");
    (moves.iter().enumerate()).any(|(n, m)| m.2 == ty && moves[..n].iter().any(|b| b.1 == m.0))
}

/// A press of the key Qt calls `qt` reached an item of type `target`.
fn key_reached(lines: &[&str], qt: &str, target: &str) -> bool {
    let (qt, target) = (format!(" qt={qt} "), format!("Some(\"{target}\")"));
    lines.iter().any(|l| {
        l.starts_with("key sym=")
            && l.contains(&qt)
            && l.contains("press=true")
            && l.ends_with(&target)
    })
}

/// The item that got the release of the key Qt calls `qt` is not the one that got its press:
/// the press moved the keyboard.
fn key_moved_focus(lines: &[&str], qt: &str) -> bool {
    let qt = format!(" qt={qt} ");
    let target = |press: &str| {
        let l = lines
            .iter()
            .find(|l| l.starts_with("key sym=") && l.contains(&qt) && l.contains(press))?;
        l.split_once(" target=")?.1.split(' ').next()
    };
    matches!((target("press=true"), target("press=false")), (Some(a), Some(b)) if a != b)
}

/// The id of the item that got the press of the key Qt calls `qt`.
fn key_target(lines: &[&str], qt: &str) -> Option<String> {
    let qt = format!(" qt={qt} ");
    let l = lines
        .iter()
        .find(|l| l.starts_with("key sym=") && l.contains(&qt) && l.contains("press=true"))?;
    Some(id_after(l, " target=")?.into())
}

/// The item `id` of the dumped item tree is of type `ty` or inside an item of that type. A
/// line of the tree is `<id> <type>/<base type>/.. .. p=Some(<parent id>) ..`.
fn is_within(tree: &str, id: &str, ty: &str) -> bool {
    let ty = format!("{ty}/");
    let mut id = id.to_string();
    loop {
        let Some(mut f) = tree
            .lines()
            .map(str::split_whitespace)
            .find(|f| f.clone().next() == Some(&id))
        else {
            return false;
        };
        if f.nth(1).is_some_and(|t| t.starts_with(&ty)) {
            return true;
        }
        match f.find_map(|f| id_after(f, "p=Some(")) {
            Some(parent) => id = parent.into(),
            None => return false,
        }
    }
}

/// The implicit size of the first `ty` item of the dumped item tree.
fn implicit_size(tree: &str, ty: &str) -> Option<(f64, f64)> {
    let of = format!("{ty}/");
    let line = tree.lines().find(|l| {
        l.split_whitespace()
            .nth(1)
            .is_some_and(|t| t == ty || t.starts_with(&of))
    })?;
    let field = |key: &str| {
        line.split_whitespace()
            .find_map(|f| f.strip_prefix(key)?.parse::<f64>().ok())
    };
    Some((field("iw=")?, field("ih=")?))
}

/// The shell got the press and the release of the key Qt calls `qt`.
fn key_came(lines: &[&str], qt: &str) -> bool {
    let qt = format!(" qt={qt} ");
    let of = |press: &str| {
        lines
            .iter()
            .any(|l| l.starts_with("key sym=") && l.contains(&qt) && l.contains(press))
    };
    of("press=true") && of("press=false")
}

/// The QML tree the tests run: the one tools/testenv/ri-gate names in RI_SHIPPED_TREE (what
/// tools/install.sh installed), else the one compiled in.
fn shipped_tree() -> PathBuf {
    std::env::var_os("RI_SHIPPED_TREE").map_or_else(|| ricompiled::ROOT.into(), PathBuf::from)
}

impl Shell {
    /// Starts impulse-shell on `file` of the shipped tree ("" is the shell itself); `config` is more
    /// members for the original's `config.json`. None, said through `testenv::unavailable`, where the
    /// tests cannot run.
    fn start(file: &str, config: &str, bus: bool) -> Option<Self> {
        Self::start_in(&shipped_tree(), file, config, bus, |_| {})
    }

    /// [`Shell::start`] on the QML tree at `tree`, in a home `furnish` has put its files in
    /// first.
    fn start_in(
        tree: &Path,
        file: &str,
        config: &str,
        bus: bool,
        furnish: impl Fn(&Path),
    ) -> Option<Self> {
        let mut need = vec!["sway", "grim", "fc-list", "stdbuf"];
        if bus {
            need.extend(["dbus-daemon", "gdbus", "notify-send"]);
        }
        if !testenv::require_all(&need) {
            return None;
        }
        // Without the original's icon font every icon is drawn as its name.
        let fonts = Command::new("fc-list")
            .arg("Material Symbols Rounded")
            .output();
        assert!(
            fonts.is_ok_and(|o| !o.stdout.is_empty()),
            "no Material Symbols Rounded font (tools/testenv/Containerfile installs it)"
        );
        assert!(tree.join("shell.qml").is_file(), "no QML tree at {tree:?}");
        let bin = std::env::var_os("RI_SHIPPED_BIN")
            .map_or_else(|| env!("CARGO_BIN_EXE_impulse-shell").into(), PathBuf::from);
        let sway = testenv::Sway::start();
        let input = Input::connect(&sway)?;
        let hypr = Hypr::listen(sway.runtime_dir());
        let home = tempfile::tempdir().unwrap();
        let conf = home.path().join(".config/impulse-shell");
        std::fs::create_dir_all(&conf).unwrap();
        // The desktop clock comes on screen whenever its shape is ready, which can be while a
        // panel opens: its pixels would count as the panel's.
        let quiet = r#""background":{"widgets":{"clock":{"enable":false}}}"#;
        let sep = if config.is_empty() { "" } else { "," };
        std::fs::write(
            conf.join("config.json"),
            format!("{{{quiet}{sep}{config}}}"),
        )
        .unwrap();
        // qml/services/FirstRunExperience.qml opens the welcome window over a home without it.
        let state = home.path().join(".local/state/rustshell/user");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join("first_run.txt"), "greeted").unwrap();
        furnish(home.path());
        let bus = bus.then(|| Bus::start(sway.runtime_dir()));
        let (r, w) = std::io::pipe().unwrap();
        let mut sh = Shell {
            log: Log::capture(r),
            _child: Kill(Command::new("true").spawn().unwrap()),
            input,
            empty: screen(&sway),
            hypr,
            path: tree.join(file),
            bin,
            bus,
            home,
            sway,
        };
        // libwayland writes each line of its log at once (see the top of this file).
        let mut cmd = Command::new("stdbuf");
        cmd.arg("-eL").arg(&sh.bin).arg("-p").arg(&sh.path);
        sh.env(&mut cmd)
            .env("WAYLAND_DEBUG", "1")
            .env("RI_DEBUG", "dump")
            .env("RI_LOG", "ri::present=debug,ri::ptr=debug,ri::keys=debug")
            .stdout(w.try_clone().unwrap())
            .stderr(w);
        sh._child = Kill(cmd.spawn().unwrap());
        drop(cmd);
        Some(sh)
    }

    /// The shell with the first frame of its bar, the layer `bar`, on screen.
    fn with_bar(config: &str, bar: &str, bus: bool) -> Option<(Self, Surface)> {
        let sh = Self::start("", config, bus)?;
        let s = sh.layer(bar, 0);
        Some((sh, s))
    }

    /// `impulse-shell -p <path>` in the shell's environment.
    fn impulse_shell(&self) -> Command {
        let mut cmd = Command::new(&self.bin);
        self.env(&mut cmd).arg("-p").arg(&self.path);
        cmd
    }

    fn env<'a>(&self, cmd: &'a mut Command) -> &'a mut Command {
        self.sway
            .env(cmd)
            .envs(testenv::home_vars(self.home.path()))
            .env("HYPRLAND_INSTANCE_SIGNATURE", Hypr::SIGNATURE)
            // The lock and the session screen hold power buttons: no logind to reach, whatever
            // gets clicked.
            .env(
                "DBUS_SYSTEM_BUS_ADDRESS",
                "unix:path=/nonexistent/ri-shipped-panels",
            )
            .env_remove("RI_INTERP")
            .env_remove("DBUS_SESSION_BUS_ADDRESS")
            .stdin(Stdio::null());
        if let Some(bus) = &self.bus {
            cmd.env("DBUS_SESSION_BUS_ADDRESS", &bus.address);
        }
        cmd
    }

    /// The log has this many lines now: what a later wait looks past.
    fn mark(&self) -> usize {
        untangled(&self.log.text()).lines().count()
    }

    /// Blocks until the log's lines from `from` on satisfy `f`; a script error ends the wait
    /// and the test.
    fn wait(&self, what: &str, from: usize, f: impl Fn(&[&str]) -> bool) {
        eprintln!("waiting for {what}");
        let r = self.log.wait(|s| {
            let s = untangled(s);
            !script_errors(KNOWN_FAULTS, &s).is_empty()
                || f(&s.lines().skip(from).collect::<Vec<_>>())
        });
        self.no_script_errors();
        if let Err(log) = r {
            panic!("impulse-shell exited before {what}:\n{}", tail(&log));
        }
    }

    /// The end of a test: the shell logged no script error, and every known fault still.
    fn end(&self) {
        self.no_script_errors();
        let log = untangled(&self.log.text());
        let (met, gone) = known_faults(KNOWN_FAULTS, &log);
        for l in met {
            eprintln!("known fault, not fixed: {l}");
        }
        let gone: Vec<String> = gone
            .iter()
            .map(|i| format!("{:?}", KNOWN_FAULTS[*i]))
            .collect();
        assert!(
            gone.is_empty(),
            "the shipped shell no longer logs these: take them off KNOWN_FAULTS\n{}",
            gone.join("\n")
        );
    }

    fn no_script_errors(&self) {
        let log = untangled(&self.log.text());
        let errors = script_errors(KNOWN_FAULTS, &log);
        assert!(
            errors.is_empty(),
            "the shipped shell logged script errors:\n{}",
            errors.join("\n")
        );
    }

    fn lines<T>(&self, from: usize, f: impl Fn(&[&str]) -> T) -> T {
        f(&untangled(&self.log.text())
            .lines()
            .skip(from)
            .collect::<Vec<_>>())
    }

    fn ipc(&self, args: &[&str]) -> String {
        let out = self
            .impulse_shell()
            .args(["ipc", "call"])
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "ipc call {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn shot(&self) -> Shot {
        screen(&self.sway)
    }

    /// Blocks until the surface `find` sees in the log from `from` on has a frame on screen.
    fn shown(&self, what: &str, from: usize, find: impl Fn(&[&str]) -> Option<Surface>) -> Surface {
        self.wait(&format!("{what} on screen"), from, |l| {
            find(l).is_some_and(|s| presented(l, &s) > 0)
        });
        self.lines(from, find).unwrap()
    }

    fn layer(&self, namespace: &str, from: usize) -> Surface {
        self.shown(namespace, from, |l| layer_surface(l, namespace))
    }

    /// Blocks until the screen inside `rect` holds a picture `before` did not have:
    /// [`MIN_PIXELS`] changed pixels in [`MIN_COLOURS`] colours. A panel fades or slides, so
    /// the screen is looked at after each frame `surface` presents from `from` on.
    fn draws(&self, surface: &Surface, from: usize, before: &Shot, rect: Rect) {
        let mut frames = 0;
        loop {
            self.wait(
                &format!("a picture in {} after its frame {frames}", surface.name),
                from,
                |l| presented(l, surface) > frames,
            );
            frames = self.lines(from, |l| presented(l, surface));
            let (pixels, colours) = self.shot().changed(before, rect);
            eprintln!(
                "{} frame {frames}: {pixels} pixels changed, in {colours} colours",
                surface.name
            );
            if pixels >= MIN_PIXELS && colours >= MIN_COLOURS {
                return;
            }
        }
    }

    /// Moves the pointer to screen point `to`, which is on `surface`, and returns where the
    /// surface's corner is on the screen.
    fn point_at(&mut self, surface: &Surface, to: (i32, i32)) -> (f64, f64) {
        let from = self.mark();
        self.input.move_to(to);
        let what = format!("the pointer on {} at {to:?}", surface.name);
        // Every line counts for where the pointer is, a new one for the move having arrived.
        let on = |l: &[&str]| pointer_on(l, surface);
        self.wait(&what, 0, |l| {
            l[from.min(l.len())..]
                .iter()
                .any(|l| l.contains("] wl_pointer#") && !l.contains(".frame("))
                && on(l).is_some()
        });
        let (x, y) = self.lines(0, on).unwrap();
        (to.0 as f64 - x, to.1 as f64 - y)
    }

    /// The file `name` of the dump the shell writes of itself now.
    fn dump(&self, name: &str) -> String {
        let files = self.ipc(&["ri-debug", "dump"]);
        let file = files
            .lines()
            .find(|l| l.ends_with(name))
            .unwrap_or_else(|| panic!("ri-debug dump wrote no {name}: {files}"));
        std::fs::read_to_string(file).unwrap()
    }

    /// The clickables of the window `window` names: a layer namespace, or `lock:`.
    fn clickables(&self, window: &str) -> Vec<Clickable> {
        self.dump("ri-click.txt")
            .lines()
            .filter_map(|l| {
                let f: Vec<&str> = l.split('\t').collect();
                let n = |i: usize| f.get(i)?.parse().ok();
                f[0].starts_with(window).then_some(Clickable {
                    rect: [n(2)?, n(3)?, n(4)?, n(5)?],
                    chain: f.get(6)?.to_string(),
                    label: f.get(7).unwrap_or(&"").to_string(),
                })
            })
            .collect()
    }

    /// Waits until the first `ty` item of the shell is fully opaque. A list view brings a new
    /// delegate in by its opacity and its scale together, and while an item is scaled it is not
    /// where the dump of clickables says: the pointer would miss it.
    fn comes_in(&self, surface: &Surface, ty: &str) {
        let what = format!("{ty} to come in fully");
        let of = format!("{ty}/");
        loop {
            let frames = self.lines(0, |l| presented(l, surface));
            let tree = self.dump("ri-tree.txt");
            let opacity = tree
                .lines()
                .map(str::split_whitespace)
                .find(|f| {
                    f.clone()
                        .nth(1)
                        .is_some_and(|t| t == ty || t.starts_with(&of))
                })
                .and_then(|mut f| f.find_map(|f| f.strip_prefix("op=")?.parse::<f64>().ok()));
            eprintln!("{ty} opacity: {opacity:?}");
            if opacity.is_some_and(|o| o >= 1.0) {
                return;
            }
            self.wait(&what, 0, |l| presented(l, surface) > frames);
        }
    }

    /// Clicks the middle of the clickable of `surface` that `is` picks; `inside` is a screen
    /// point on the surface.
    fn click(&mut self, surface: &Surface, inside: (i32, i32), is: impl Fn(&Clickable) -> bool) {
        let rect = |sh: &Self| {
            let all = sh.clickables(&surface.name);
            all.iter()
                .find(|c| is(c))
                .map(|c| c.rect)
                .unwrap_or_else(|| {
                    panic!(
                        "{} has no such thing to click, only:\n{all:#?}",
                        surface.name
                    )
                })
        };
        let (x, y) = self.point_at(surface, inside);
        let mut at = rect(self);
        // A row of toggles animates its widths when it comes in, and the dump says where a
        // thing is now: the press goes where two dumps in a row, one before the pointer moved
        // there and one after, found it.
        let mut over = inside;
        loop {
            let [cx, cy, w, h] = at;
            let to = ((x + cx + w / 2.0) as i32, (y + cy + h / 2.0) as i32);
            // sway reports no motion to the point the pointer is at already.
            if to != over {
                self.point_at(surface, to);
                over = to;
            }
            let now = rect(self);
            if now == at {
                return self.press(surface, to);
            }
            at = now;
        }
    }

    /// Clicks the screen point `to`, which is on `surface`.
    fn click_at(&mut self, surface: &Surface, to: (i32, i32)) {
        self.point_at(surface, to);
        self.press(surface, to);
    }

    /// Presses and releases the button over the screen point `to` of `surface`, where the
    /// pointer is.
    fn press(&mut self, surface: &Surface, to: (i32, i32)) {
        let from = self.mark();
        self.input.button(wl_pointer::ButtonState::Pressed);
        self.input.button(wl_pointer::ButtonState::Released);
        // The press, not the release: a press that closes the panel takes its surface along.
        self.wait(
            &format!("the click at {to:?} to reach {}", surface.name),
            from,
            |l| l.iter().any(|l| l.starts_with("ptr press")),
        );
    }

    /// Opens the panel `namespace` through its IPC target and returns it once it has drawn
    /// inside `rect`.
    fn open(&mut self, namespace: &str, target: &str, rect: Rect) -> Surface {
        let before = self.shot();
        let from = self.mark();
        self.ipc(&[target, "open"]);
        let s = self.layer(namespace, from);
        self.draws(&s, from, &before, rect);
        s
    }

    /// Blocks until the window that got the keyboard after line `from` is active: a key
    /// pressed from then on reaches its items.
    fn active(&self, from: usize) {
        // Qt Quick sends a key only to `activeFocusItem`, which a window has once the reply to
        // the `wl_display.sync` after `wl_keyboard.enter` has activated it; so does impulse-shell.
        self.wait("the window with the keyboard to be active", from, |l| {
            l.iter()
                .any(|l| l.starts_with("kbd target ") && l.contains(" -> Some("))
        });
    }

    /// [`Shell::open`] for a panel that takes the keyboard: returns it once its window is
    /// active too.
    fn open_for_keys(&mut self, namespace: &str, target: &str, rect: Rect) -> Surface {
        let from = self.mark();
        let s = self.open(namespace, target, rect);
        self.active(from);
        s
    }

    /// Presses the key `code`, which Qt calls `qt`, until an item of type `within`, or one
    /// inside such an item, gets it. A press that reached another item is followed by the
    /// keyboard moving on, as `what` says it should: the next press comes after that.
    fn key_reaches(&mut self, what: &str, (code, qt): (u32, &str), within: &str) {
        loop {
            let from = self.mark();
            self.input.key(code, false);
            self.wait(&format!("{what}: a key to reach the shell"), from, |l| {
                key_came(l, qt)
            });
            let got = self.lines(from, |l| key_target(l, qt)).unwrap();
            if is_within(&self.dump("ri-tree.txt"), &got, within) {
                return;
            }
            self.wait(what, from, focus_given);
        }
    }

    /// Blocks until impulse-shell has closed `surface`, which `by` should make it do.
    fn closes(&self, surface: &Surface, from: usize, by: &str) {
        self.wait(&format!("{by} to close {}", surface.name), from, |l| {
            destroyed(l, surface)
        });
    }

    /// Presses Escape and blocks until `surface` is closed.
    fn escape_closes(&mut self, surface: &Surface) {
        let from = self.mark();
        self.input.key(KEY_ESC, false);
        self.closes(surface, from, "Escape");
    }
}

fn screen(sway: &testenv::Sway) -> Shot {
    let out = sway
        .env(&mut Command::new("grim"))
        .args(["-t", "ppm", "-"])
        .output()
        .expect("grim");
    assert!(out.status.success(), "grim");
    let b = out.stdout;
    // P6\n<w> <h>\n255\n<rgb>: the pixels follow the fourth whitespace-separated field.
    let mut i = 0;
    for _ in 0..4 {
        while b[i].is_ascii_whitespace() {
            i += 1;
        }
        while !b[i].is_ascii_whitespace() {
            i += 1;
        }
    }
    let shot = Shot(b[i + 1..].as_chunks::<3>().0.to_vec());
    assert_eq!(
        shot.0.len(),
        (SCREEN.0 * SCREEN.1) as usize,
        "grim's picture"
    );
    shot
}

/// The screen under the horizontal bar.
const DESK_TOP: usize = 45;

#[test]
fn right_sidebar_opens_draws_and_takes_a_click_and_a_key() {
    let Some((mut sh, _bar)) = Shell::with_bar("", "rustshell:bar", false) else {
        return;
    };
    let s = sh.open_for_keys(
        "rustshell:sidebarRight",
        "sidebarRight",
        (1470, DESK_TOP, 1920, 1080),
    );
    let from = sh.mark();
    sh.click(&s, (1700, 500), |c| c.label == "Keep awake");
    sh.wait("the Keep awake toggle to inhibit idling", from, |l| {
        l.iter()
            .any(|l| l.contains(".create_inhibitor(new id zwp_idle_inhibitor_v1#"))
    });
    sh.escape_closes(&s);
    sh.end();
}

/// The frames of `surface` on screen since the compositor told it the output's scale is
/// `scale` 120ths.
fn presented_at_scale(lines: &[&str], surface: &Surface, scale: u32) -> usize {
    let of = format!(", wl_surface#{})", surface.wl);
    let Some(id) = lines
        .iter()
        .filter(|l| l.contains(".get_fractional_scale(new id ") && l.ends_with(&of))
        .find_map(|l| id_after(l, "new id wp_fractional_scale_v1#"))
    else {
        return 0;
    };
    let told = format!("wp_fractional_scale_v1#{id}.preferred_scale({scale})");
    match lines.iter().position(|l| l.contains(&told)) {
        Some(i) => presented(&lines[i..], surface),
        None => 0,
    }
}

/// At output scale 1.25 the lower edge of the button with the month's name over the right
/// sidebar's calendar is in the middle of a device row (538 * 1.25 = 672.5), and the button
/// clips its content. The original under Quickshell 0.3.2 leaves that row to the sidebar;
/// a clip rounded up to the next row puts one more row of the button's colour there.
#[test]
fn calendar_month_button_leaves_its_half_row_to_the_sidebar_at_scale_1_25() {
    const SCALE: f64 = 1.25;
    let Some((mut sh, _bar)) = Shell::with_bar("", "rustshell:bar", false) else {
        return;
    };
    let from = sh.mark();
    let ok = Command::new("swaymsg")
        .env("SWAYSOCK", sh.sway.ipc_socket())
        .arg("output HEADLESS-1 scale 1.25")
        .stdout(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    assert!(ok, "swaymsg scale");
    sh.wait("the output's scale to reach the shell", from, |l| {
        l.iter().any(|l| l.contains(".preferred_scale(150)"))
    });
    let s = sh.open(
        "rustshell:sidebarRight",
        "sidebarRight",
        (1470, DESK_TOP, 1920, 1080),
    );
    sh.wait("the sidebar drawn at scale 1.25", 0, |l| {
        presented_at_scale(l, &s, 150) > 0
    });
    // The pointer tells where the surface is; it stays off the calendar.
    let to = (1700, 300);
    let (dx, dy) = sh.point_at(&s, to);
    let local = (to.0 as f64 - dx, to.1 as f64 - dy);
    let corner = (to.0 as f64 / SCALE - local.0, to.1 as f64 / SCALE - local.1);
    let month = |sh: &Shell| {
        let all = sh.clickables(&s.name);
        all.iter()
            .filter(|c| c.chain.contains("CalendarHeaderButton"))
            .map(|c| c.rect)
            .max_by(|a, b| a[2].total_cmp(&b[2]))
            .unwrap_or_else(|| panic!("no calendar header button in:\n{all:#?}"))
    };
    // The sidebar slides in: the picture is taken once the button and its pixels stand still.
    let (mut rect, mut shot) = (month(&sh), sh.shot());
    loop {
        let (r, p) = (month(&sh), sh.shot());
        let still = r == rect && p.0 == shot.0;
        (rect, shot) = (r, p);
        if still {
            break;
        }
    }
    let [x, y, w, h] = rect;
    let left = (corner.0 + x) * SCALE;
    let right = (corner.0 + x + w) * SCALE;
    let bottom = (corner.1 + y + h) * SCALE;
    eprintln!("month button {rect:?} at {corner:?}: device x {left}..{right}, bottom {bottom}");
    assert_eq!(
        bottom.fract(),
        0.5,
        "the button's lower edge is no longer in the middle of a device row: this test needs \
         another such edge"
    );
    let bottom = bottom.floor() as usize;
    // Between the round ends: the button's radius is half its height.
    let ends = (h * SCALE / 2.0).ceil();
    let cols = (left + ends) as usize..(right - ends) as usize;
    assert!(cols.len() > 20, "the button is wider than its round ends");
    let px = |x: usize, y: usize| shot.0[y * SCREEN.0 as usize + x];
    for x in cols {
        let (button, sidebar) = (px(x, bottom - 2), px(x, bottom + 3));
        assert_ne!(button, sidebar, "the button has a colour of its own at {x}");
        assert_eq!(px(x, bottom - 1), button, "its last row at {x}");
        assert_eq!(px(x, bottom), sidebar, "the row under it at {x}");
    }
    sh.end();
}

#[cfg(not(feature = "interp"))]
fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_tree(&e.path(), &target);
        } else {
            std::fs::copy(e.path(), target).unwrap();
        }
    }
}

/// `Rustshell.reload()` (the right sidebar's "Reload Hyprland & shell") on a tree that is no
/// longer the one compiled in, `change` having been made to it: the shell stays as it is and
/// its `ReloadPopup.qml` shows why, which `Rustshell.reloadFailed` told it. A build with the
/// interpreter runs such a tree, so these are tests of the build without it.
#[cfg(not(feature = "interp"))]
fn reload_is_refused_and_shown(change: impl FnOnce(&Path)) {
    let from = shipped_tree();
    let dir = tempfile::tempdir().unwrap();
    let tree = dir.path().join("impulse-shell");
    copy_tree(&from, &tree);
    let Some(mut sh) = Shell::start_in(&tree, "", "", false, |_| {}) else {
        return;
    };
    let bar = sh.layer("rustshell:bar", 0);
    let s = sh.open_for_keys(
        "rustshell:sidebarRight",
        "sidebarRight",
        (1470, DESK_TOP, 1920, 1080),
    );
    change(&tree);
    let from = sh.mark();
    sh.click(&s, (1700, 500), |c| c.label == "restart_alt");
    let popup = sh.layer("rustshell:reloadPopup", from);
    let (frames, said) = sh.lines(from, |l| {
        let said: Vec<String> = l
            .iter()
            .filter(|l| l.starts_with("ERROR: ") || l.contains("Shell not reloaded"))
            .map(|l| l.to_string())
            .collect();
        (presented(l, &popup), said)
    });
    assert!(frames > 0);
    assert_eq!(
        said.first().map(String::as_str),
        Some(
            format!(
                "ERROR: {} differs from the tree this shell was built from. The running shell \
                 keeps its current code.",
                tree.display()
            )
            .as_str()
        ),
        "{said:#?}"
    );
    assert_eq!(
        said.last().map(String::as_str),
        Some("ERROR: Install again: tools/install.sh"),
        "{said:#?}"
    );
    assert!(
        !said.iter().any(|l| l.contains("Shell not reloaded")),
        "{said:#?}"
    );
    // The tree that was running: its bar and its sidebar are the surfaces they were.
    assert!(
        sh.lines(from, |l| !destroyed(l, &bar) && !destroyed(l, &s)),
        "the running shell did not stay"
    );
    sh.ipc(&["sidebarRight", "close"]);
    sh.end();
}

#[cfg(not(feature = "interp"))]
#[test]
fn a_reload_on_an_edited_tree_keeps_the_shell_and_shows_why() {
    reload_is_refused_and_shown(|tree| {
        let file = tree.join("modules/common/models/AnimatedTabIndexPair.qml");
        let was = std::fs::read_to_string(&file).unwrap();
        let edited = was.replace(
            "property int idx2Duration: 300",
            "property int idx2Duration: root.idx1Duration * 4",
        );
        assert_ne!(edited, was, "{file:?} changed; update the edit");
        std::fs::write(file, edited).unwrap();
    });
}

#[cfg(not(feature = "interp"))]
#[test]
fn a_reload_on_a_tree_that_lacks_a_file_keeps_the_shell_and_shows_why() {
    reload_is_refused_and_shown(|tree| {
        std::fs::remove_file(tree.join("modules/rusty/bar/Bar.qml")).unwrap();
    });
}

#[test]
fn left_sidebar_opens_draws_and_takes_a_click_and_a_key() {
    // With one tab the sidebar has no tab bar to click.
    let config = r#""sidebar":{"translator":{"enable":true}}"#;
    let Some((mut sh, _bar)) = Shell::with_bar(config, "rustshell:bar", false) else {
        return;
    };
    let s = sh.open_for_keys(
        "rustshell:sidebarLeft",
        "sidebarLeft",
        (0, DESK_TOP, 450, 1080),
    );
    sh.click(&s, (200, 500), |c| c.label == "Translator");
    // The page the tab brings in takes the keyboard for its text.
    sh.key_reaches(
        "the Translator page to take the keyboard",
        (KEY_END, "0x1000011"),
        "Translator",
    );
    sh.escape_closes(&s);
    sh.end();
}

#[test]
fn overview_opens_draws_and_takes_a_click_and_a_key() {
    let Some((mut sh, _bar)) = Shell::with_bar("", "rustshell:bar", false) else {
        return;
    };
    // The grid of workspaces, under the search box: a Loader makes it, and a frame can come
    // with the box alone, when the overview's input region ends above the grid too.
    let rect = (66, 150, 1854, 510);
    let s = sh.open("rustshell:overview", "search", rect);
    // A press on a workspace of the grid goes there and closes the overview.
    let from = sh.mark();
    sh.click_at(&s, (700, 200));
    sh.hypr.wait("the pressed workspace to be focused", |r| {
        r.contains("hl.dsp.focus({ workspace = ")
    });
    sh.closes(&s, from, "the press on a workspace");

    let s = sh.open_for_keys("rustshell:overview", "search", rect);
    let before = sh.lines(0, |l| input_heights(l, &s));
    sh.input.key(KEY_A, false);
    // The search results come under the grid: the overview grows.
    sh.wait("the search to list results for the typed letter", 0, |l| {
        key_reached(l, "0x41", "ToolbarTextField")
            && input_heights(l, &s)[before.len()..]
                .iter()
                .any(|h| h > before.last().unwrap_or(&0))
    });
    sh.escape_closes(&s);
    sh.end();
}

#[test]
fn session_screen_opens_draws_and_takes_a_click_and_a_key() {
    let Some((mut sh, _bar)) = Shell::with_bar("", "rustshell:bar", false) else {
        return;
    };
    let s = sh.open_for_keys("rustshell:session", "session", (480, 240, 1440, 840));
    let from = sh.mark();
    sh.input.key(KEY_RIGHT, false);
    sh.wait("Right to move to the next action", from, |l| {
        key_moved_focus(l, "0x1000014")
    });
    // Beside the action buttons: they suspend, log out and power off.
    let from = sh.mark();
    sh.click_at(&s, (100, 1000));
    sh.closes(&s, from, "a click beside the actions");
    sh.end();
}

#[test]
fn lock_screen_opens_draws_and_takes_a_click_and_a_key() {
    let Some((mut sh, _bar)) = Shell::with_bar("", "rustshell:bar", false) else {
        return;
    };
    let before = sh.shot();
    let from = sh.mark();
    sh.ipc(&["lock", "activate"]);
    sh.wait("sway to lock the session", from, |l| {
        l.iter()
            .any(|l| l.contains("] ext_session_lock_v1#") && l.ends_with(".locked()"))
    });
    let s = sh.shown("the lock surface", from, |l| {
        role_surface(
            l,
            "lock:",
            ".get_lock_surface(",
            "ext_session_lock_surface_v1",
        )
    });
    // Its toolbar: user, password field, power buttons.
    sh.draws(&s, from, &before, (640, 980, 1280, 1080));
    sh.active(from);
    // The confirm button, the one in the toolbar that neither suspends nor powers off.
    let from = sh.mark();
    sh.click(&s, (960, 540), |c| {
        c.chain.starts_with("MouseArea<ToolbarButton<")
    });
    // The click gave the button the keyboard and started the check of the password. The button
    // is disabled while that runs and its focus scope has the keyboard, as in Qt
    // (QQuickItemPrivate::setEffectiveEnableRecur): a key pressed meanwhile goes to the scope,
    // above the surface's key handler, and does nothing. Enabled again, the button takes the
    // keyboard back, and the surface sends a key pressed anywhere on it back to the password
    // field.
    sh.wait(
        "the keyboard to come back to the clicked button after the password check",
        from,
        |l| focus_back(l, "ToolbarButton"),
    );
    let from = sh.mark();
    sh.input.key(KEY_A, false);
    sh.wait(
        "a typed letter to go from the clicked button back to the password field",
        from,
        |l| key_reached(l, "0x41", "ToolbarButton") && key_moved_focus(l, "0x41"),
    );
    sh.end();
}

/// A notification popup asks the compositor for no keyboard, so there is no key to press in
/// it: the test fails once that changes.
#[test]
fn notification_popup_opens_draws_and_takes_a_click() {
    let Some((mut sh, _bar)) = Shell::with_bar("", "rustshell:bar", true) else {
        return;
    };
    // The shell takes the notification server's name after its bar is on screen, and this bus
    // starts no service for a name nobody has: gdbus returns once the name has an owner.
    let owned = sh
        .env(&mut Command::new("gdbus"))
        .args(["wait", "--session", "org.freedesktop.Notifications"])
        .status()
        .expect("gdbus");
    assert!(owned.success(), "gdbus wait: {owned}");
    let before = sh.shot();
    let from = sh.mark();
    let sent = sh
        .env(&mut Command::new("notify-send"))
        .args(["Gate", "a notification for the shipped shell"])
        .status()
        .expect("notify-send");
    assert!(sent.success(), "notify-send: {sent}");
    let s = sh.layer("rustshell:notificationPopup", from);
    sh.draws(&s, from, &before, (1480, DESK_TOP, 1920, 240));
    assert!(
        sh.lines(0, |l| takes_no_keyboard(l, &s)),
        "the notification popup takes the keyboard now: press a key in it here"
    );
    sh.comes_in(&s, "NotificationGroup");
    let expand = |c: &Clickable| c.chain.contains("NotificationGroupExpandButton");
    // Expanding makes the popup taller, collapsing shorter again; coming on screen only grows.
    let seen = sh.lines(0, |l| input_heights(l, &s));
    sh.click(&s, (1700, 80), expand);
    sh.wait("the notification to expand", 0, |l| {
        input_heights(l, &s)[seen.len()..]
            .iter()
            .any(|h| h > seen.last().unwrap_or(&0))
    });
    let seen = sh.lines(0, |l| input_heights(l, &s));
    sh.click(&s, (1700, 80), expand);
    sh.wait("the notification to collapse", 0, |l| {
        input_heights(l, &s)[seen.len()..]
            .iter()
            .any(|h| h < seen.iter().max().unwrap())
    });
    sh.end();
}

/// The bar asks for no keyboard either.
#[test]
fn vertical_bar_opens_draws_and_takes_a_click() {
    let config = r#""bar":{"vertical":true}"#;
    let Some((mut sh, bar)) = Shell::with_bar(config, "rustshell:verticalBar", false) else {
        return;
    };
    let from = sh.mark();
    sh.ipc(&["bar", "close"]);
    sh.closes(&bar, from, "bar close");
    let bar = sh.open("rustshell:verticalBar", "bar", (0, 0, 69, 1080));
    let before = sh.shot();
    let from = sh.mark();
    sh.click(&bar, (30, 600), |c| c.chain.contains("<LeftSidebarButton<"));
    let side = sh.layer("rustshell:sidebarLeft", from);
    sh.draws(&side, from, &before, (70, 0, 520, 1080));
    assert!(
        sh.lines(0, |l| takes_no_keyboard(l, &bar)),
        "the vertical bar takes the keyboard now: press a key in it here"
    );
    sh.end();
}

/// The settings window is a process of its own, as `ShellControl.openWindow` starts it.
#[test]
fn settings_window_opens_draws_and_takes_a_click_and_a_key() {
    let Some(mut sh) = Shell::start("settings.qml", "", false) else {
        return;
    };
    let s = sh.shown("the settings window", 0, |l| {
        role_surface(l, "settings", ".get_xdg_surface(", "xdg_surface")
    });
    sh.draws(&s, 0, &sh.empty, (0, 0, 1920, 1080));
    sh.active(0);
    // Right of the page list: a page that changes is a picture that changes there.
    let page = (200, 100, 1900, 1060);
    let (before, from) = (sh.shot(), sh.mark());
    sh.click_at(&s, (95, 238));
    eprintln!("the General page, clicked in the page list");
    sh.draws(&s, from, &before, page);
    // Ctrl+F is the window's own shortcut for its search field.
    let from = sh.mark();
    sh.input.key(KEY_F, true);
    sh.input.key(KEY_A, false);
    sh.wait(
        "Ctrl+F to give the search field the next letter",
        from,
        |l| key_reached(l, "0x41", "StyledTextInput"),
    );
    sh.end();
}

/// The three pictures of [`floating_image_plays_its_gif`]'s file.
const GIF_COLOURS: [[u8; 3]; 3] = [[255, 0, 0], [0, 255, 0], [0, 0, 255]];
/// The side of a picture on screen, and of the file: the widget shows it at a twentieth.
const GIF_SIDE: u16 = 200;
const GIF_FILE_SIDE: u16 = 4000;

/// Which of [`GIF_COLOURS`] fills a picture's worth of the screen, if one does.
fn gif_picture(shot: &Shot) -> Option<usize> {
    let near = |a: [u8; 3], b: [u8; 3]| (0..3).all(|c| a[c].abs_diff(b[c]) < 40);
    let half = usize::from(GIF_SIDE) * usize::from(GIF_SIDE) / 2;
    (0..GIF_COLOURS.len()).find(|&k| {
        shot.0
            .iter()
            .filter(|px| near(**px, GIF_COLOURS[k]))
            .count()
            >= half
    })
}

/// The overlay's Floating Image widget, the tree's one `AnimatedImage` (`asynchronous: true`,
/// `playing: visible`, `loops` and `cache` left alone), plays the GIF its config names: every
/// picture of the file comes on screen. The file is local, so nothing is fetched; the screen
/// is looked at after each frame the overlay presents. A movie that never opens (the item
/// stayed `Loading` and drew nothing) or shows its first picture only leaves the test waiting.
///
/// Nothing but the movie itself may bring it on screen. An item nobody laid out again when
/// its decode landed was still drawn by whatever laid the overlay out next: the opening, if
/// the decode was quick, or the taskbar's clock at the next minute. So the file is big (a
/// decode of all of it outlasts the opening) and the clock shows the year.
///
/// The widget copies the file into the original's fixed `/tmp/rustshell/media/images`, which every
/// shell removes when it starts: no other test runs meanwhile (`.config/nextest.toml`).
#[test]
fn floating_image_plays_its_gif() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("play.gif");
    let mut e = gif::Encoder::new(
        std::fs::File::create(&file).unwrap(),
        GIF_FILE_SIDE,
        GIF_FILE_SIDE,
        GIF_COLOURS.as_flattened(),
    )
    .unwrap();
    e.set_repeat(gif::Repeat::Infinite).unwrap();
    for k in 0..GIF_COLOURS.len() {
        let frame = gif::Frame {
            width: GIF_FILE_SIDE,
            height: GIF_FILE_SIDE,
            // Hundredths of a second: a picture stays for several frames of the screen.
            delay: 20,
            buffer: vec![k as u8; usize::from(GIF_FILE_SIDE) * usize::from(GIF_FILE_SIDE)].into(),
            ..Default::default()
        };
        e.write_frame(&frame).unwrap();
    }
    drop(e);
    let config = format!(
        r#""time":{{"format":"yyyy"}},"overlay":{{"floatingImage":{{"imageSource":"file://{}","scale":{}}}}}"#,
        file.display(),
        f64::from(GIF_SIDE) / f64::from(GIF_FILE_SIDE)
    );
    // qml/modules/common/Persistent.qml: the widgets the overlay opens with.
    let furnish = |home: &Path| {
        std::fs::write(
            home.join(".local/state/rustshell/states.json"),
            r#"{"overlay":{"open":["floatingImage"],"floatingImage":{"x":700,"y":300}}}"#,
        )
        .unwrap();
    };
    let Some(sh) = Shell::start_in(&shipped_tree(), "", &config, false, furnish) else {
        return;
    };
    sh.layer("rustshell:bar", 0);
    let from = sh.mark();
    sh.ipc(&["overlay", "toggle"]);
    let overlay = sh.layer("rustshell:overlay", from);
    let mut seen = [false; GIF_COLOURS.len()];
    let mut frames = 0;
    while seen.contains(&false) {
        sh.wait(
            &format!("the overlay's frame after {frames}, pictures seen: {seen:?}"),
            from,
            |l| presented(l, &overlay) > frames,
        );
        frames = sh.lines(from, |l| presented(l, &overlay));
        if let Some(k) = gif_picture(&sh.shot()) {
            if !seen.contains(&true) {
                // The picture is drawn, so the item took its size: laid out again once the
                // decode landed, not only painted from the frames the decode left behind.
                assert_eq!(implicit_size(&sh.dump("ri-tree.txt"), "AnimatedImage"), {
                    let side = f64::from(GIF_SIDE);
                    Some((side, side))
                });
            }
            seen[k] = true;
        }
    }
    sh.end();
}

#[test]
fn a_wayland_line_inside_a_shell_line_is_taken_out() {
    let log = "qml: [12:00:00.000001]  -> wl_surface#3.commit()\n\
               [12:00:00.000002] wl_callback#4.done(1)\n\
               ClipboardHistory: listing failed with code 1\n\
               focus win=Some(1) None -> Some(2)\n";
    assert_eq!(
        untangled(log).lines().collect::<Vec<_>>(),
        [
            "[12:00:00.000001]  -> wl_surface#3.commit()",
            "[12:00:00.000002] wl_callback#4.done(1)",
            "qml: ClipboardHistory: listing failed with code 1",
            "focus win=Some(1) None -> Some(2)",
        ]
    );
    assert!(script_errors(KNOWN_FAULTS, &untangled(log)).is_empty());
}

#[test]
fn a_known_fault_is_named_and_missed_but_is_no_script_error() {
    let faults: Faults = &[(
        "qml: binding loop detected on item ",
        " property collapsedWidth (NavigationRailTabArray in ",
    )];
    let log = "qml: binding loop detected on item 1290 property collapsedWidth \
               (NavigationRailTabArray in /impulse-shell/modules/common/widgets/NavigationRailTabArray.qml)\n\
               qml: binding loop detected on item 7 property width (Row in /impulse-shell/Bar.qml)\n";
    assert_eq!(
        script_errors(faults, log),
        ["qml: binding loop detected on item 7 property width (Row in /impulse-shell/Bar.qml)"]
    );
    let (met, gone) = known_faults(faults, log);
    assert_eq!(met.len(), 1);
    assert!(gone.is_empty());
    let (met, gone) = known_faults(
        faults,
        "qml: ClipboardHistory: listing failed with code 1\n",
    );
    assert!(met.is_empty());
    assert_eq!(gone, [0]);
}
