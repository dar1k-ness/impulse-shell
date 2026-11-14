//! `QT_SCALE_FACTOR` and `QT_SCREEN_SCALE_FACTORS`, the real Quickshell (Qt 6) vs the native
//! shell on the same config, in a sway of the test's own with two outputs: HEADLESS-1 at
//! scale 1 and HEADLESS-2 at the fractional 1.5. Qt multiplies every screen's and window's
//! scale by `QT_SCALE_FACTOR` times the screen's entry of `QT_SCREEN_SCALE_FACTORS`
//! (QHighDpiScaling, qhighdpiscaling.cpp): QML keeps its sizes, the compositor gets a surface,
//! margins, an exclusive zone and popup positions that many times larger, the screen reads
//! that many times smaller and the pointer's position is divided by it.
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};

use testenv::Log;
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, delegate_noop};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

const NATIVE: &str = env!("CARGO_BIN_EXE_rusty-impulse");

/// The outputs side by side, in sway's layout pixels: 1920x1080 and 2560x1600 at 1.5.
const LAYOUT: (u32, u32) = (1920 + 1707, 1080);

/// A bar with margins and an exclusive zone on every screen, a popup under it, and a window
/// that sway puts on the second output: grey until `t.paint` over IPC makes them red, green
/// and blue. `t.dump` logs what QML sees.
/// A right margin of [`RIGHT`] keeps the bar a whole number of device pixels wide on the 1.5
/// output with every factor here.
fn fixture(pragmas: &str, right: u32) -> String {
    fixture_with(pragmas, right, true)
}

/// [`fixture`], with the popups shown or not.
fn fixture_with(pragmas: &str, right: u32, popups: bool) -> String {
    format!(
        r##"//@ pragma Env QS_NO_RELOAD_POPUP=1
{pragmas}import QtQuick
import Quickshell
import Quickshell.Io
ShellRoot {{
    id: root
    signal dump()
    property bool painted: false
    IpcHandler {{
        target: "t"
        function paint(): void {{
            console.log("PAINT NOW")
            root.painted = true
        }}
        function dump(): void {{
            root.dump()
            console.log("DUMPED")
        }}
    }}
    Variants {{
        model: Quickshell.screens
        PanelWindow {{
            id: p
            required property var modelData
            screen: modelData
            anchors {{ top: true; left: true; right: true }}
            margins {{ left: 20; right: {right} }}
            implicitHeight: 40
            exclusiveZone: 40
            color: root.painted ? "#ff0000" : "#101010"
            MouseArea {{
                anchors.fill: parent
                hoverEnabled: true
                onPositionChanged: m => console.log("MOUSE " + p.screen.name + " " + m.x + " " + m.y)
                onExited: console.log("LEFT " + p.screen.name)
            }}
            PopupWindow {{
                anchor.window: p
                anchor.rect.x: 100
                anchor.rect.y: 40
                implicitWidth: 50
                implicitHeight: 30
                visible: {popups}
                color: root.painted ? "#00ff00" : "#101010"
            }}
            Connections {{
                target: root
                function onDump() {{
                    console.log("SEEN bar " + p.screen.name + " " + p.width + "x" + p.height
                        + " ratio " + p.devicePixelRatio + " screen " + p.screen.x + "," + p.screen.y
                        + " " + p.screen.width + "x" + p.screen.height
                        + " ratio " + p.screen.devicePixelRatio)
                }}
            }}
            Component.onCompleted: console.log("READY")
        }}
    }}
    FloatingWindow {{
        id: f
        width: 300
        height: 200
        color: root.painted ? "#0000ff" : "#101010"
        Connections {{
            target: root
            function onDump() {{
                console.log("SEEN window " + f.width + "x" + f.height + " ratio " + f.devicePixelRatio)
            }}
        }}
        Component.onCompleted: console.log("READY")
    }}
}}
"##
    )
}

fn swaymsg(sway: &testenv::Sway, args: &[&str]) -> String {
    let out = Command::new("swaymsg")
        .env("SWAYSOCK", sway.ipc_socket())
        .args(args)
        .output()
        .expect("swaymsg");
    assert!(out.status.success(), "swaymsg {args:?}: {out:?}");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// The sway with the second output, at 1.5, right of the first.
fn two_outputs() -> testenv::Sway {
    let sway = testenv::Sway::start();
    for cmd in [
        "create_output",
        "output HEADLESS-1 position 0 0",
        "output HEADLESS-2 resolution 2560x1600",
        "output HEADLESS-2 scale 1.5",
        "output HEADLESS-2 position 1920 0",
        "default_border none",
        "for_window [app_id=\"org.quickshell\"] floating enable",
        "for_window [app_id=\"org.rustshell\"] floating enable",
    ] {
        swaymsg(&sway, &[cmd]);
    }
    sway
}

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
            name, interface, ..
        } = ev
        {
            match interface.as_str() {
                "wl_seat" if g.seat.is_none() => g.seat = Some(reg.bind(name, 1, qh, ())),
                "zwlr_virtual_pointer_manager_v1" => g.vptr = Some(reg.bind(name, 1, qh, ())),
                _ => {}
            }
        }
    }
}

delegate_noop!(G: ignore wl_seat::WlSeat);
delegate_noop!(G: ZwlrVirtualPointerManagerV1);
delegate_noop!(G: ZwlrVirtualPointerV1);

/// A pointer over the whole layout.
struct Pointer {
    q: EventQueue<G>,
    g: G,
    ptr: ZwlrVirtualPointerV1,
    time: u32,
}

impl Pointer {
    fn new(sway: &testenv::Sway) -> Pointer {
        let conn = Connection::from_socket(UnixStream::connect(sway.display()).unwrap()).unwrap();
        let mut q = conn.new_event_queue();
        let qh = q.handle();
        conn.display().get_registry(&qh, ());
        let mut g = G::default();
        q.roundtrip(&mut g).unwrap();
        let ptr = (g.vptr.as_ref().expect("zwlr_virtual_pointer_manager_v1"))
            .create_virtual_pointer(g.seat.as_ref(), &qh, ());
        Pointer { q, g, ptr, time: 0 }
    }

    /// To the layout's pixel `x`,`y`.
    fn move_to(&mut self, x: u32, y: u32) {
        self.time += 10;
        self.ptr
            .motion_absolute(self.time, x, y, LAYOUT.0, LAYOUT.1);
        self.ptr.frame();
        self.q.roundtrip(&mut self.g).unwrap();
    }
}

/// `output` as RGB rows, in device pixels.
fn shot(sway: &testenv::Sway, dir: &Path, output: &str) -> Option<(usize, usize, Vec<u8>)> {
    let f = dir.join("shot.ppm");
    let ok = sway
        .env(&mut Command::new("grim"))
        .args(["-t", "ppm", "-o", output])
        .arg(&f)
        .status()
        .ok()?
        .success();
    if !ok {
        return None;
    }
    let b = std::fs::read(&f).ok()?;
    // P6\n<w> <h>\n255\n<rgb>
    let mut fields = Vec::new();
    let mut i = 0;
    while fields.len() < 4 {
        while b.get(i)?.is_ascii_whitespace() {
            i += 1;
        }
        let s = i;
        while !b.get(i)?.is_ascii_whitespace() {
            i += 1;
        }
        fields.push(String::from_utf8_lossy(&b[s..i]).into_owned());
    }
    let (w, h): (usize, usize) = (fields[1].parse().ok()?, fields[2].parse().ok()?);
    Some((w, h, b[i + 1..].to_vec()))
}

/// Where the pixels of exactly `color` are: `x y width height`, or `none`.
fn bounds(img: &(usize, usize, Vec<u8>), color: [u8; 3]) -> String {
    let (w, h, px) = img;
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    for y in 0..*h {
        for x in 0..*w {
            if px[(y * w + x) * 3..][..3] == color {
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
            }
        }
    }
    if x0 == usize::MAX {
        return "none".into();
    }
    format!("{x0} {y0} {} {}", x1 - x0 + 1, y1 - y0 + 1)
}

const COLORS: [(&str, [u8; 3]); 3] = [
    ("bar", [255, 0, 0]),
    ("popup", [0, 255, 0]),
    ("window", [0, 0, 255]),
];

/// What is on both outputs: each color's bounds in device pixels.
fn on_screen(sway: &testenv::Sway, dir: &Path) -> Vec<String> {
    let mut all = Vec::new();
    for output in ["HEADLESS-1", "HEADLESS-2"] {
        let img = shot(sway, dir, output).expect("grim");
        for (name, color) in COLORS {
            all.push(format!("{output} {name} at {}", bounds(&img, color)));
        }
    }
    all
}

/// The workspaces' rectangles: what the bars' exclusive zones leave.
fn workspaces(sway: &testenv::Sway) -> Vec<String> {
    let text = swaymsg(sway, &["-t", "get_workspaces", "-r"]);
    let flat: String = text.split_whitespace().collect();
    flat.split("\"rect\":")
        .skip(1)
        .filter_map(|r| Some(format!("workspace {}", r.split_once('}')?.0)))
        .collect()
}

/// The surfaces with a role in a `WAYLAND_DEBUG` log, as `wl_surface#N.`: the bars, the
/// popups and the window.
fn surfaces(log: &str) -> Vec<String> {
    let mut all = Vec::new();
    for l in log.lines() {
        if !(l.contains(".get_layer_surface(") || l.contains(".get_xdg_surface(")) {
            continue;
        }
        let id = l
            .split("wl_surface#")
            .nth(1)
            .and_then(|r| r.split(')').next());
        let id = id.and_then(|r| r.split(',').next());
        if let Some(id) = id {
            let s = format!("wl_surface#{id}.");
            if !all.contains(&s) {
                all.push(s);
            }
        }
    }
    all
}

/// The five surfaces are on their outputs and were told the scale to draw at.
fn placed(log: &str) -> bool {
    let all = surfaces(log);
    all.len() == 5
        && all.iter().all(|s| {
            // `get_fractional_scale(new id wp_fractional_scale_v1#N, wl_surface#S)`
            let of = format!(", {})", s.trim_end_matches('.'));
            let scale = log.lines().find_map(|l| {
                let (_, r) = l.split_once(".get_fractional_scale(new id ")?;
                Some(format!("{}.preferred_scale(", r.strip_suffix(&of)?))
            });
            log.contains(&format!("{s}enter(")) && scale.is_none_or(|e| log.contains(&e))
        })
}

/// Every surface's frame drawn after `t.paint` is on screen: the compositor answered what the
/// surface asked for after the call, a frame callback (Quickshell) or a presentation feedback
/// (rusty-impulse under `RI_LOG=ri::present`). Either's id is reused only once it is answered, so the
/// first answer after the request is its own.
fn painted(log: &str) -> bool {
    let lines: Vec<&str> = log.lines().collect();
    let Some(from) = lines.iter().rposition(|l| l.contains("PAINT NOW")) else {
        return false;
    };
    let id = |line: &str, request: &str| {
        let (_, r) = line.split_once(request)?;
        r.split(')').next().map(str::to_string)
    };
    surfaces(log).iter().all(|s| {
        let s = s.trim_end_matches('.');
        let frame = format!("{s}.frame(new id wl_callback#");
        let feedback = format!(".feedback({s}, new id wp_presentation_feedback#");
        (from + 1..lines.len()).any(|i| {
            let later = &lines[i + 1..];
            if let Some(id) = id(lines[i], &frame) {
                let done = format!("wl_callback#{id}.done(");
                later.iter().any(|l| l.contains(&done))
            } else if let Some(id) = id(lines[i], &feedback) {
                let presented = format!("wp_presentation_feedback#{id}.presented(");
                let discarded = format!("wp_presentation_feedback#{id}.discarded(");
                later
                    .iter()
                    .find(|l| l.contains(&presented) || l.contains(&discarded))
                    .is_some_and(|l| l.contains(&presented))
            } else {
                false
            }
        })
    })
}

/// `t.<function>` over IPC to the `bin` that serves `dir`.
fn call(bin: &str, sway: &testenv::Sway, dir: &Path, function: &str) {
    let mut call = Command::new(bin);
    let call = sway
        .env(&mut call)
        .args(["ipc", "-p"])
        .arg(dir)
        .args(["call", "t", function])
        .env("RI_INTERP", "1")
        .output()
        .unwrap();
    assert!(call.status.success(), "{bin}: {call:?}");
}

/// Everything `bin` shows and sees with `env` set in the session: the surfaces on both outputs,
/// the space left to windows, QML's sizes and ratios, and where QML sees the pointer put 50,10
/// into each output.
fn seen(bin: &str, pragmas: &str, env: &[(&str, &str)]) -> Vec<String> {
    seen_with(bin, pragmas, env, RIGHT)
}

const RIGHT: u32 = 40;

/// `bin` started on the fixture in `dir` with `env` set in the session, and its log.
fn start(
    bin: &str,
    sway: &testenv::Sway,
    dir: &Path,
    env: &[(&str, &str)],
) -> (Kill, std::sync::Arc<Log>) {
    let (rd, wr) = std::io::pipe().unwrap();
    let mut c = Command::new(bin);
    sway.env(&mut c)
        .arg("-p")
        .arg(dir)
        .env_remove("QT_SCALE_FACTOR")
        .env_remove("QT_SCREEN_SCALE_FACTORS")
        .env("RI_INTERP", "1")
        .env("WAYLAND_DEBUG", "1")
        .env("RI_LOG", "ri::present=debug")
        .envs(env.iter().copied())
        .stdin(Stdio::null())
        .stdout(wr.try_clone().unwrap())
        .stderr(wr);
    let child = c.spawn().unwrap();
    drop(c);
    (Kill(child), Log::capture(rd))
}

/// The lines of a shell's log that name one of the two variables.
fn named(log: &str) -> Vec<&str> {
    log.lines().filter(|l| l.contains("QT_SC")).collect()
}

/// [`seen`] with the bars' right margin at `right`.
fn seen_with(bin: &str, pragmas: &str, env: &[(&str, &str)], right: u32) -> Vec<String> {
    seen_log(bin, pragmas, env, right).0
}

/// [`seen_with`], and the shell's log.
fn seen_log(bin: &str, pragmas: &str, env: &[(&str, &str)], right: u32) -> (Vec<String>, String) {
    let sway = two_outputs();
    // The window opens on the output with the pointer and the focus: the second.
    let mut pointer = Pointer::new(&sway);
    pointer.move_to(1920 + 800, 900);
    swaymsg(&sway, &["focus output HEADLESS-2"]);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("shell.qml"), fixture(pragmas, right)).unwrap();
    let (_k, log) = start(bin, &sway, dir.path(), env);
    let wait = |what: &str, f: &dyn Fn(&str) -> bool| {
        log.wait(f)
            .unwrap_or_else(|l| panic!("{bin} with {env:?} exited before {what}:\n{l}"));
    };
    wait("it loaded", &|l| l.matches("READY").count() >= 3);
    // Every surface is on its output at that output's scale; what it draws from now on is
    // what stays.
    wait("its surfaces were on their outputs", &placed);
    call(bin, &sway, dir.path(), "paint");
    // Not in the configuration's directory: a file written there reloads it.
    let shots = tempfile::tempdir().unwrap();
    // A frame Quickshell's render thread began before the call may be the one answered: the
    // painted one's answer is then a later line.
    let mut read = 0;
    let shown = loop {
        wait("its surfaces were painted", &|l| {
            l.len() > read && painted(l)
        });
        read = log.text().len();
        let now = on_screen(&sway, shots.path());
        if now.iter().filter(|l| !l.ends_with("none")).count() >= 5 {
            break now;
        }
    };
    for (x, name) in [(0, "HEADLESS-1"), (1920, "HEADLESS-2")] {
        // Right of the largest left margin here: 20 at a factor of 3. Then off the bar, to
        // where nothing is: the last position the bar saw is the second.
        pointer.move_to(x + 295, 8);
        pointer.move_to(x + 300, 10);
        pointer.move_to(x + 10, 1000);
        wait("the pointer was over a bar and left it", &|l| {
            l.contains(&format!("LEFT {name}"))
        });
    }
    call(bin, &sway, dir.path(), "dump");
    wait("it said what it sees", &|l| l.contains("DUMPED"));
    let text = log.text();
    let mut said: Vec<String> = text
        .lines()
        .filter_map(|l| Some(format!("qml {}", l.split_once("SEEN ")?.1.trim())))
        .collect();
    said.sort();
    for name in ["HEADLESS-1", "HEADLESS-2"] {
        let mark = format!("MOUSE {name} ");
        let at = text.lines().rev().find_map(|l| l.split_once(&mark));
        said.push(format!("pointer on {name} at {}", at.unwrap().1.trim()));
    }
    let mut all = shown;
    all.extend(workspaces(&sway));
    all.extend(said);
    (all, text)
}

/// What Quickshell's QML sees with `env` set in the session, where Qt shows no window to
/// wait for: the bars' lines of `t.dump`, sorted. Qt says nothing about the two variables.
/// Without the popups: Qt asks for one of no size, which is a protocol error that ends
/// Quickshell.
fn quickshell_sees(env: &[(&str, &str)]) -> Vec<String> {
    let sway = two_outputs();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("shell.qml"), fixture_with("", RIGHT, false)).unwrap();
    let (_k, log) = start("qs", &sway, dir.path(), env);
    let wait = |what: &str, f: &dyn Fn(&str) -> bool| {
        log.wait(f)
            .unwrap_or_else(|l| panic!("qs with {env:?} exited before {what}:\n{l}"));
    };
    wait("it loaded", &|l| l.matches("READY").count() >= 3);
    call("qs", &sway, dir.path(), "dump");
    wait("it said what it sees", &|l| l.contains("DUMPED"));
    let text = log.text();
    assert_eq!(named(&text), [""; 0], "Quickshell with {env:?}");
    let mut bars: Vec<String> = text
        .lines()
        .filter_map(|l| Some(l.split_once("SEEN bar ")?.1.trim().to_string()))
        .collect();
    bars.sort();
    bars
}

fn ready() -> bool {
    testenv::require_all(&["qs", "sway", "swaymsg", "grim"])
}

/// The popup of the bar on the second screen, which Quickshell misplaces once that screen
/// has a factor: [`a_popup_on_another_screen_stays_at_its_anchor`].
const OTHER_POPUP: &str = "HEADLESS-2 popup at ";

/// Both shells with `env` in the session show and see the same.
#[track_caller]
fn same(pragmas: &str, env: &[(&str, &str)]) {
    same_with(pragmas, env, RIGHT);
}

#[track_caller]
fn same_with(pragmas: &str, env: &[(&str, &str)], right: u32) {
    let rest = |all: Vec<String>| -> Vec<String> {
        (all.into_iter())
            .filter(|l| !l.starts_with(OTHER_POPUP))
            .collect()
    };
    let (want, quickshell) = seen_log("qs", pragmas, env, right);
    let (got, native) = seen_log(NATIVE, pragmas, env, right);
    // Neither says a word about the variables.
    assert_eq!(named(&quickshell), [""; 0], "Quickshell with {env:?}");
    assert_eq!(named(&native), [""; 0], "the native shell with {env:?}");
    let (want, got) = (rest(want), rest(got));
    let differing: Vec<String> = want
        .iter()
        .zip(&got)
        .filter(|(w, g)| w != g)
        .map(|(w, g)| format!("  quickshell {w}\n  native     {g}"))
        .collect();
    assert!(
        differing.is_empty() && want.len() == got.len(),
        "with {env:?} in the session and the pragmas [{pragmas}]:\n{}\nquickshell: {want:#?}\nnative: {got:#?}",
        differing.join("\n")
    );
}

/// Without the variables: the outputs' own scales, 1 and 1.5.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn without_the_variables_the_outputs_scales_apply() {
    if ready() {
        same("", &[]);
    }
}

/// A window's color fills its buffer: a bar 1661 wide on the 1.5 output is 2491.5 device
/// pixels wide, on a buffer of 2492, and with a factor of 2.5 a surface 1556 wide is a window
/// 622 wide, which is 2332.5 device pixels on a buffer of 2334.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_window_s_color_fills_its_buffer() {
    if ready() {
        same_with("", &[], 25);
        same_with("", &[("QT_SCREEN_SCALE_FACTORS", ";2.5")], RIGHT);
    }
}

/// `QT_SCALE_FACTOR` multiplies every screen's scale: whole, fractional and below 1.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn qt_scale_factor_scales_every_screen() {
    if ready() {
        for v in ["2", "1.5", "1.3", "0.5", " 2 "] {
            same("", &[("QT_SCALE_FACTOR", v)]);
        }
    }
}

/// Text that is no number leaves the scale at 1.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_qt_scale_factor_that_is_no_number_is_one() {
    if ready() {
        for v in ["abc", "2,5", "2x", "", "1e999", "infinity"] {
            same("", &[("QT_SCALE_FACTOR", v)]);
        }
    }
}

/// The other variables of Qt's scaling are not read: with each the native shell shows what
/// it shows without. Quickshell takes them: a DPI of 192 doubles every screen, and `Round`
/// makes 1 and 2 of the entries 1.3 and 1.6.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn the_other_scale_variables_are_not_read() {
    if !ready() {
        return;
    }
    let want = seen_with(NATIVE, "", &[], RIGHT);
    for env in [
        ("QT_FONT_DPI", "192"),
        ("QT_WAYLAND_FORCE_DPI", "192"),
        ("QT_WAYLAND_FORCE_DPI", "physical"),
        ("QT_USE_PHYSICAL_DPI", "1"),
        ("QT_ENABLE_HIGHDPI_SCALING", "0"),
        ("QT_SCALE_FACTOR_ROUNDING_POLICY", "Round"),
    ] {
        assert_eq!(seen_with(NATIVE, "", &[env], RIGHT), want, "{env:?}");
    }
    for name in ["QT_FONT_DPI", "QT_WAYLAND_FORCE_DPI"] {
        let bars = quickshell_sees(&[(name, "192")]);
        assert_eq!(ratios(&bars), [["2", "2"], ["3", "4"]], "{name}: {bars:?}");
    }
    let entries = ("QT_SCREEN_SCALE_FACTORS", "1.3;1.6");
    let round = ("QT_SCALE_FACTOR_ROUNDING_POLICY", "Round");
    let bars = quickshell_sees(&[entries, round]);
    // The window on the output at 1.5 is at 2 times 2 as well.
    assert_eq!(ratios(&bars), [["1", "1"], ["4", "4"]], "{bars:?}");
    assert_eq!(
        seen_with(NATIVE, "", &[entries, round], RIGHT),
        seen_with(NATIVE, "", &[entries], RIGHT)
    );
}

/// `QT_SCREEN_SCALE_FACTORS` scales each screen by its own entry: by position, by name, with
/// entries that are no number dropped and factors below 1 read as 1.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn qt_screen_scale_factors_scale_each_screen() {
    if ready() {
        for v in [
            "1.5;2",
            "2",
            "HEADLESS-2=1.5",
            "HEADLESS-1=2;1.5",
            "abc;2",
            "1;1",
            "0.5;0.5",
        ] {
            same("", &[("QT_SCREEN_SCALE_FACTORS", v)]);
        }
    }
}

/// An entry of `QT_SCREEN_SCALE_FACTORS` that is no number above 0 is dropped without a
/// word: the entries after it move up a screen, and a screen named by it keeps 1.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_screen_factor_that_is_no_number_above_zero_is_dropped() {
    if ready() {
        for v in ["0", "-1", "", "abc", "nan", "-inf", "1e999"] {
            same("", &[("QT_SCREEN_SCALE_FACTORS", &format!("{v};2"))]);
        }
        for v in ["0", "-1", "abc", "nan"] {
            let named = format!("HEADLESS-1={v};HEADLESS-2=2");
            same("", &[("QT_SCREEN_SCALE_FACTORS", &named)]);
        }
    }
}

/// Both variables multiply.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn the_two_variables_multiply() {
    if ready() {
        same(
            "",
            &[
                ("QT_SCALE_FACTOR", "2"),
                ("QT_SCREEN_SCALE_FACTORS", "1.5;1.25"),
            ],
        );
    }
}

/// `//@ pragma Env QT_SCALE_FACTOR=1` (the three windows' configs) overrides the session's
/// factor; a commented-out pragma (shell.qml) leaves it.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn the_pragma_overrides_the_session_s_factor() {
    if ready() {
        let session = [("QT_SCALE_FACTOR", "2")];
        same("//@ pragma Env QT_SCALE_FACTOR=1\n", &session);
        same("////@ pragma Env QT_SCALE_FACTOR=1\n", &session);
    }
}

/// A popup of a window on a screen that is not the first: Qt scales a position around its
/// screen's origin and keeps the popup's window on the first screen, so with a factor
/// Quickshell puts the popup at the output's left edge, in the first screen's factor. The
/// native shell keeps it at its anchor, in its parent's.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_popup_on_another_screen_stays_at_its_anchor() {
    if !ready() {
        return;
    }
    let popup = |bin: &str, env: &[(&str, &str)]| -> String {
        let all = seen(bin, "", env);
        let l = all.iter().find(|l| l.starts_with(OTHER_POPUP)).unwrap();
        l[OTHER_POPUP.len()..].to_string()
    };
    // Without a factor both put it under the bar, 100 right of the bar's left edge.
    assert_eq!(popup("qs", &[]), popup(NATIVE, &[]));
    // The first screen's 1.5 in Quickshell, the popup's own screen's 2 in the native shell.
    let twice = [("QT_SCALE_FACTOR", "2")];
    let each = [("QT_SCREEN_SCALE_FACTORS", "1.5;2")];
    let got = [
        popup("qs", &twice),
        popup(NATIVE, &twice),
        popup("qs", &each),
        popup(NATIVE, &each),
    ];
    assert_eq!(
        got,
        [
            "0 120 150 89",
            "360 120 150 89",
            "0 120 113 67",
            "360 120 150 89"
        ]
    );
}

/// The bars' ratios in [`quickshell_sees`]: the windows' and the screens', HEADLESS-1 first.
fn ratios(bars: &[String]) -> Vec<Vec<&str>> {
    bars.iter()
        .map(|bar| {
            let after = bar.split(" ratio ").skip(1);
            after.map(|r| r.split(' ').next().unwrap()).collect()
        })
        .collect()
}

/// A `QT_SCALE_FACTOR` that is a number but not a finite one above 0. Qt takes it as the
/// factor of every window and screen and shows no window (a bar of the size 2147483647 at
/// 0), without a word. The native shell shows what it shows without the variable and says
/// why in one line that names the variable and its value.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_qt_scale_factor_no_window_can_have_is_one_and_one_warning() {
    if !ready() {
        return;
    }
    let want = seen(NATIVE, "", &[]);
    // The value, then Qt's ratios on HEADLESS-1 and of HEADLESS-2's window (1.5 times the
    // factor) and screen (2 times).
    for (v, qt, window, screen) in [
        ("0", "0", "0", "0"),
        ("-1", "-1", "-1.5", "-2"),
        ("nan", "NaN", "NaN", "NaN"),
        ("inf", "Infinity", "Infinity", "Infinity"),
        ("-inf", "-Infinity", "-Infinity", "-Infinity"),
    ] {
        let env = [("QT_SCALE_FACTOR", v)];
        let bars = quickshell_sees(&env);
        assert_eq!(ratios(&bars), [[qt, qt], [window, screen]], "{v}: {bars:?}");
        let (got, log) = seen_log(NATIVE, "", &env, RIGHT);
        assert_eq!(got, want, "{v}");
        let said = named(&log);
        let line = format!("WARN: QT_SCALE_FACTOR={v} ");
        assert!(said.len() == 1 && said[0].contains(&line), "{v}: {said:?}");
    }
}

/// An infinite entry of `QT_SCREEN_SCALE_FACTORS`. Qt takes it as the factor of its screen
/// and shows no window there, without a word. The native shell drops the entry as it drops
/// the ones that are no number above 0, and says why in one line that names the variable
/// and the entry.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn an_infinite_screen_factor_is_dropped_and_one_warning() {
    if !ready() {
        return;
    }
    for (v, without, entry, qt) in [
        ("inf;2", "2", "inf", [["Infinity", "Infinity"], ["3", "4"]]),
        (
            "HEADLESS-1=inf;HEADLESS-2=2",
            "HEADLESS-2=2",
            "HEADLESS-1=inf",
            [["Infinity", "Infinity"], ["3", "4"]],
        ),
    ] {
        let env = [("QT_SCREEN_SCALE_FACTORS", v)];
        let bars = quickshell_sees(&env);
        assert_eq!(ratios(&bars), qt, "{v}: {bars:?}");
        let want = seen(NATIVE, "", &[("QT_SCREEN_SCALE_FACTORS", without)]);
        let (got, log) = seen_log(NATIVE, "", &env, RIGHT);
        assert_eq!(got, want, "{v}");
        let said = named(&log);
        let line = format!("WARN: QT_SCREEN_SCALE_FACTORS: the entry {entry} ");
        assert!(said.len() == 1 && said[0].contains(&line), "{v}: {said:?}");
    }
}
