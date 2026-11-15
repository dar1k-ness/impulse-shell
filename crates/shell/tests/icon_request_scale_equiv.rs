//! An Image whose source is `image://icon/` (IconImage, `Quickshell.iconPath`, a notification's
//! and a tray item's icon) against Quickshell 0.3.2 on a headless sway, by the Image's implicit
//! size (the pixmap's pixels over the Image's own devicePixelRatio) and by the pixel it draws.
//!
//! Qt 6.12 and Quickshell, as measured here:
//! - IconImageProvider::requestPixmap calls `QIcon::pixmap(w, h)` without a ratio, so QIcon
//!   takes QGuiApplication's (qicon.cpp:939-952): the highest wl_output.scale of all outputs.
//!   QIconLoaderEngine::scaledPixmap (qiconloader.cpp:999-1004) picks the directory for the
//!   requested size at that scale, and the pixmap is the requested size times it (an SVG
//!   fitted, a PNG never enlarged). A request without a size is 100x100, one with a zero side
//!   2x2.
//! - QQuickImageBase::loadPixmap requests `sourceSize` times the Image's own ratio, which is
//!   its window's, or the application's for an Image loaded before its window exists (those a
//!   shell declares); without `sourceSize` the request has no size and the ratio is 1. The
//!   implicit size is the pixmap's pixels over that ratio.
//! - A Kirigami.Icon asks the provider itself (Icon::findIcon) for its own size, so its pixmap
//!   is that size times the application's ratio as well, whatever its window's.
//! - An Image is loaded again when its window's ratio changes (on a fractional output: from
//!   the output's integer scale to the fraction), not when an output comes or goes: the icon
//!   shown stays, a new Image of the same source and size gets the cached pixmap, one of
//!   another size the new scale.
//!
//! The theme tells the directory and the scale by the icon's shape and colour: `x` is an SVG
//! in Scalable directories of scales 1 to 3 whose size ranges do not overlap, `g` a PNG in
//! Fixed directories, `p` one 40 pixel PNG.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use qsg::raster::Canvas;
use testenv::Log;

/// The size ranges of `x`'s Scalable directories, at each scale.
const RANGES: [(i32, i32); 5] = [(1, 30), (31, 45), (46, 60), (61, 90), (91, 400)];
/// The height of `x` (32 wide) in each directory, by scale: no two alike.
const HEIGHTS: [[i32; 5]; 3] = [
    [30, 26, 22, 18, 14],
    [29, 27, 25, 23, 21],
    [28, 24, 20, 19, 17],
];
/// The sizes of `g`'s Fixed directories, at each scale.
const FIXED: [i32; 5] = [16, 20, 24, 32, 48];

fn colour(scale: usize, n: usize, blue: u8) -> [u8; 4] {
    [(60 * scale + 15) as u8, (40 * n + 40) as u8, blue, 255]
}

fn write_png(file: &Path, w: usize, h: usize, c: [u8; 4]) {
    Canvas::new(w, h, c)
        .save_png(file.to_str().unwrap())
        .unwrap();
}

/// The theme `r` under `share`, with an `icon-theme.cache` when `cache`.
fn write_theme(share: &Path, cache: bool) {
    let t = share.join("icons/r");
    let (mut plain, mut scaled) = (vec!["one".to_string()], Vec::new());
    let mut index = String::from("\n[one]\nSize=40\nType=Fixed\n");
    std::fs::create_dir_all(t.join("one")).unwrap();
    write_png(&t.join("one/p.png"), 40, 40, [255, 255, 255, 255]);
    for s in 1..=3usize {
        for (n, (min, max)) in RANGES.iter().enumerate() {
            let d = format!("s{s}r{n}");
            index += &format!(
                "\n[{d}]\nSize={min}\nScale={s}\nType=Scalable\nMinSize={min}\nMaxSize={max}\n"
            );
            std::fs::create_dir_all(t.join(&d)).unwrap();
            let h = HEIGHTS[s - 1][n];
            // `x`, and the icon impulse-shell draws for a name no theme has.
            for (name, blue) in [("x", 255), ("application-x-executable", 128)] {
                let [r, g, b, _] = colour(s, n, blue);
                std::fs::write(
                    t.join(&d).join(format!("{name}.svg")),
                    format!(
                        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"32\" height=\"{h}\">\
                         <rect width=\"32\" height=\"{h}\" fill=\"#{r:02x}{g:02x}{b:02x}\"/></svg>\n"
                    ),
                )
                .unwrap();
            }
            // `e`: an edge in the middle, which a pixmap of fewer pixels than the screen's blurs.
            let [r, g, b, _] = colour(s, n, 255);
            std::fs::write(
                t.join(&d).join("e.svg"),
                format!(
                    "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"32\" height=\"{h}\">\
                     <rect width=\"32\" height=\"{h}\" fill=\"#ffffff\"/>\
                     <rect width=\"16\" height=\"{h}\" fill=\"#{r:02x}{g:02x}{b:02x}\"/></svg>\n"
                ),
            )
            .unwrap();
            if s == 1 { &mut plain } else { &mut scaled }.push(d);
        }
        for (n, size) in FIXED.iter().enumerate() {
            let d = format!("f{size}s{s}");
            index += &format!("\n[{d}]\nSize={size}\nScale={s}\nType=Fixed\n");
            std::fs::create_dir_all(t.join(&d)).unwrap();
            let side = *size as usize * s;
            write_png(
                &t.join(&d).join("g.png"),
                side,
                side - n - 1,
                colour(s, n, 0),
            );
            if s == 1 { &mut plain } else { &mut scaled }.push(d);
        }
    }
    std::fs::write(
        t.join("index.theme"),
        format!(
            "[Icon Theme]\nName=r\nDirectories={}\nScaledDirectories={}\n{index}",
            plain.join(","),
            scaled.join(",")
        ),
    )
    .unwrap();
    if cache {
        let st = Command::new("gtk-update-icon-cache")
            .args(["-f", "-q"])
            .arg(&t)
            .status()
            .expect("gtk-update-icon-cache");
        assert!(st.success(), "gtk-update-icon-cache");
    }
}

/// The panel every output gets. `//QS` is where Quickshell's copy tells its frames.
const SCENE: &str = r##"
import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Widgets
import Quickshell.Services.SystemTray
import Quickshell.Services.Notifications
import org.kde.kirigami as Kirigami

ShellRoot {
    id: root
    property int gen: 0
    property int tick: 0
    signal dump()
    NotificationServer {
        id: notes
        onNotification: n => {
            n.tracked = true;
            console.log("NOTE " + n.appIcon);
        }
    }
    IpcHandler {
        target: "p"
        function more(): void { root.gen++; console.log("GEN " + root.gen); }
        function tick(): void { root.tick++; console.log("TICK " + root.tick); }
        function dump(): void { root.dump(); }
    }
    Variants {
        model: Quickshell.screens
        PanelWindow {
            id: p
            required property var modelData
            readonly property string out: modelData.name
            screen: modelData
            anchors { top: true; left: true }
            implicitWidth: 760
            implicitHeight: 60
            color: "transparent"
            function say(n, i) {
                console.log("IMG " + p.out + " " + n + " " + i.implicitWidth + "x" + i.implicitHeight + " " + i.status);
            }
            function each(of) {
                for (let k = 0; k < of.children.length; k++)
                    if (of.children[k].n)
                        p.say(of.children[k].n, of.children[k]);
            }
            Connections {
                target: root
                function onDump() {
                    p.say("sized", a); p.say("unsized", b); p.say("IconImage", c.backer);
                    p.say("fallback", f); p.say("width-only", z); p.say("png", g);
                    p.say("small-png", pp); p.say("missing", m);
                    console.log("IMG " + p.out + " kirigami " + k.paintedWidth + "x" + k.paintedHeight + " painted");
                    p.each(later); p.each(tray); p.each(noted); p.each(early);
                    p.say(lazy.item.n, lazy.item);
                    const got = [() => b.sourceSize, () => b.sourceSize.width, () => b.sourceSize.height,
                        () => z.sourceSize.width, () => z.sourceSize.height,
                        () => a.sourceSize, () => wide.sourceSize.height, () => m.sourceSize.width,
                        () => typeof b.sourceSize.height, () => follows.width].map(f => {
                            try { return String(f()); } catch (e) { return "throws"; }
                        });
                    console.log("UNSET " + p.out + " " + got.join(" "));
                    console.log("DUMPED " + p.out + " " + root.tick);
                }
            }
            onDevicePixelRatioChanged: console.log("DPR " + p.out + " " + p.devicePixelRatio)
            Rectangle { anchors.fill: parent; color: "#202020" }
            Rectangle { width: 4; height: 4; color: root.tick % 2 ? "#ff0000" : "#00ff00" }
            Image { id: a; x: 20; y: 20; width: 26; height: 26; sourceSize: Qt.size(26, 26); source: "image://icon/x" }
            Image { id: b; x: 60; y: 20; width: 26; height: 26; source: "image://icon/x" }
            IconImage { id: c; x: 100; y: 20; implicitSize: 26; source: Quickshell.iconPath("x") }
            Image { id: f; x: 140; y: 20; width: 26; height: 26; sourceSize: Qt.size(26, 26); source: Quickshell.iconPath("nope", "x") }
            Image { id: z; x: 180; y: 20; width: 26; height: 26; sourceSize.width: 20; source: "image://icon/x" }
            Image { id: g; x: 220; y: 20; width: 26; height: 26; sourceSize: Qt.size(16, 16); source: "image://icon/g" }
            Image { id: pp; x: 260; y: 20; width: 26; height: 26; sourceSize: Qt.size(26, 26); source: "image://icon/p" }
            // Asks at 20 wide and at the height the icon then has; and one that asks nothing.
            Image { id: wide; visible: false; sourceSize.width: 20; source: "image://icon/x" }
            Item { id: follows; width: b.sourceSize.width + wide.sourceSize.height }
            Image { id: m; x: 300; y: 20; width: 26; height: 26; sourceSize: Qt.size(26, 26); source: "image://icon/nope" }
            // Kirigami asks the provider itself, for the item's size.
            Kirigami.Icon { id: k; x: 640; y: 20; width: 26; height: 26; source: "image://icon/e" }
            Item {
                id: later
                x: 340; y: 20
                Repeater {
                    model: root.gen
                    Image {
                        required property int index
                        property string n: "later" + index
                        x: 40 * index; width: 26; height: 26
                        // The third asks for what the second of an output at 2 asked for.
                        readonly property int s: index == 2 ? 54 : 26 + index
                        sourceSize: Qt.size(s, s)
                        source: "image://icon/x"
                    }
                }
            }
            Item {
                id: tray
                x: 500; y: 20
                Repeater {
                    model: SystemTray.items
                    Image {
                        required property var modelData
                        property string n: "tray-" + modelData.id
                        width: 26; height: 26
                        sourceSize: Qt.size(26, 26)
                        source: modelData.icon
                        Component.onCompleted: console.log("TRAY " + p.out + " " + modelData.id)
                    }
                }
            }
            Item {
                id: noted
                x: 700; y: 20
                Repeater {
                    model: notes.trackedNotifications
                    Image {
                        required property var modelData
                        property string n: "notification"
                        width: 26; height: 26
                        sourceSize: Qt.size(26, 26)
                        source: Quickshell.iconPath(modelData.appIcon, "image-missing")
                    }
                }
            }
            Component {
                id: made
                Image {
                    property string n
                    y: 20; width: 26; height: 26
                    source: "image://icon/x"
                }
            }
            Item { id: early }
            Loader {
                id: lazy
                active: false
                sourceComponent: Image {
                    property string n: "early-loader"
                    x: 460; y: 20; width: 26; height: 26
                    sourceSize: Qt.size(33, 33)
                    source: "image://icon/x"
                }
            }
            Component.onCompleted: {
                // Made once the window is there, before the compositor gave it a scale.
                made.createObject(early, { n: "early-made", x: 420, sourceSize: Qt.size(31, 31) });
                lazy.active = true;
                Qt.callLater(() => made.createObject(early, { n: "early-called", x: 580, sourceSize: Qt.size(32, 32) }));
                console.log("DPR " + p.out + " " + p.devicePixelRatio);
                console.log("READY " + p.out);
            }
            Component.onDestruction: console.log("GONE " + p.out)
            //QS
        }
    }
}
"##;

/// Quickshell's own part of [`SCENE`]: `SHOWN` once the third frame after a tick is swapped.
/// A frame is swapped once the compositor showed the one before it, so by then the first
/// frame drawn after the tick is on screen.
const QS_FRAMES: &str = r##"Item {
                id: fs
                property int n: -1
                Connections {
                    target: root
                    function onTickChanged() { fs.n = 0; fs.Window.window.update(); }
                }
                Connections {
                    target: fs.Window.window
                    function onFrameSwapped() {
                        if (fs.n < 0)
                            return;
                        fs.n++;
                        if (fs.n == 3) {
                            fs.n = -1;
                            console.log("SHOWN " + p.out + " " + root.tick);
                        } else {
                            fs.Window.window.update();
                        }
                    }
                }
            }"##;

/// Where [`SCENE`] draws what it names (x of the 26x26 item at y 20), for the ones compared by
/// their pixel. `missing` is compared by its size alone: Quickshell draws a checkerboard,
/// impulse-shell the theme's generic icon (see `missing_icon_is_the_generic_one_a_found_icon_would_be`);
/// so is `width-only`, a 2x2 icon stretched, whose every pixel is an edge.
const PLACES: [(&str, usize); 13] = [
    ("sized", 20),
    ("unsized", 60),
    ("IconImage", 100),
    ("fallback", 140),
    ("png", 220),
    ("small-png", 260),
    ("later0", 340),
    ("later1", 380),
    ("early-made", 420),
    ("early-loader", 460),
    ("early-called", 580),
    ("kirigami", 640),
    ("notification", 700),
];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Quickshell,
    Ours,
}

fn swaymsg(sway: &testenv::Sway, cmd: &str) {
    let st = Command::new("swaymsg")
        .env("SWAYSOCK", sway.ipc_socket())
        .arg(cmd)
        .stdout(Stdio::null())
        .status()
        .expect("swaymsg");
    assert!(st.success(), "swaymsg {cmd}");
}

/// The surfaces whose commit after the last line holding `mark` is on screen: the
/// wp_presentation_feedback impulse-shell asked for it (under `RI_LOG=ri::present`) was `presented`.
fn surfaces_shown_after(log: &str, mark: &str) -> usize {
    let lines: Vec<&str> = log.lines().collect();
    let Some(from) = lines.iter().rposition(|l| l.contains(mark)) else {
        return 0;
    };
    let mut shown = HashSet::new();
    for i in from + 1..lines.len() {
        let Some((surface, id)) = lines[i]
            .split_once("-> wp_presentation#")
            .and_then(|(_, r)| r.split_once(".feedback(wl_surface#"))
            .and_then(|(_, r)| r.split_once(", new id wp_presentation_feedback#"))
            .and_then(|(s, r)| Some((s, r.split(')').next()?)))
        else {
            continue;
        };
        let (presented, discarded) = (
            format!("wp_presentation_feedback#{id}.presented("),
            format!("wp_presentation_feedback#{id}.discarded("),
        );
        let end = lines[i + 1..]
            .iter()
            .find(|l| l.contains(&presented) || l.contains(&discarded));
        if end.is_some_and(|l| l.contains(&presented)) {
            shown.insert(surface);
        }
    }
    shown.len()
}

/// One shell on a sway of its own.
struct Session {
    kind: Kind,
    sway: testenv::Sway,
    child: Child,
    dir: tempfile::TempDir,
    log: Arc<Log>,
    /// Each output's name and scale.
    outputs: Vec<(String, f64)>,
    tick: u32,
    gen_: u32,
}

impl Session {
    /// The shell started on outputs of `scales`, once every panel is up at its output's scale.
    fn start(kind: Kind, scales: &[f64]) -> Session {
        let sway = testenv::Sway::start();
        let mut outputs = Vec::new();
        for (n, scale) in scales.iter().enumerate() {
            // Configured before it exists: it comes up at its scale.
            let name = format!("HEADLESS-{}", n + 1);
            swaymsg(&sway, &format!("output {name} resolution 2560x1600"));
            swaymsg(&sway, &format!("output {name} scale {scale}"));
            if n > 0 {
                swaymsg(&sway, "create_output");
            }
            outputs.push((name, *scale));
        }
        let dir = tempfile::tempdir().unwrap();
        let scene = match kind {
            Kind::Quickshell => SCENE.replace("//QS", QS_FRAMES),
            Kind::Ours => SCENE.to_string(),
        };
        std::fs::write(dir.path().join("shell.qml"), scene).unwrap();
        let (r, w) = std::io::pipe().unwrap();
        let mut cmd = Self::program(kind);
        sway.env(&mut cmd)
            .arg("-p")
            .arg(dir.path())
            .stdin(Stdio::null())
            .stdout(w.try_clone().unwrap())
            .stderr(w);
        match kind {
            Kind::Quickshell => cmd
                .env("QT_QPA_PLATFORM", "wayland")
                .env("LIBGL_ALWAYS_SOFTWARE", "1")
                .env("QT_WAYLAND_DISABLE_WINDOWDECORATION", "1"),
            Kind::Ours => cmd
                .env("WAYLAND_DEBUG", "1")
                .env("RI_LOG", "ri::present=debug"),
        };
        match *FACTOR.lock().unwrap() {
            Some(factor) => cmd.env("QT_SCALE_FACTOR", factor),
            None => cmd.env_remove("QT_SCALE_FACTOR"),
        };
        let s = Session {
            kind,
            child: cmd.spawn().unwrap(),
            sway,
            dir,
            log: Log::capture(r),
            outputs,
            tick: 0,
            gen_: 0,
        };
        for (name, scale) in &s.outputs {
            s.up(name, *scale);
        }
        s
    }

    fn program(kind: Kind) -> Command {
        match kind {
            Kind::Quickshell => Command::new("qs"),
            Kind::Ours => {
                let mut c = Command::new(env!("CARGO_BIN_EXE_impulse-shell"));
                c.env("RI_INTERP", "1");
                c
            }
        }
    }

    fn wait(&self, what: &str, f: impl Fn(&str) -> bool) {
        // Named in the output nextest shows when its slow-timeout ends a hang.
        eprintln!("{:?}: waiting for {what}", self.kind);
        // wayland-backend logs `iface#id`, its discarded-message lines `iface@id`.
        if let Err(s) = self.log.wait(|s| f(&s.replace('@', "#"))) {
            let from = s.lines().count().saturating_sub(60);
            let tail: Vec<&str> = s.lines().skip(from).collect();
            panic!(
                "{:?} exited before {what}; its log tail:\n{}",
                self.kind,
                tail.join("\n")
            );
        }
    }

    /// The panel of output `name` is up and its window has the output's `scale` times [`FACTOR`].
    fn up(&self, name: &str, scale: f64) {
        let scale = scale * factor();
        let (ready, dpr) = (format!("READY {name}"), format!("DPR {name} {scale}"));
        self.wait(&format!("the panel on {name} at {scale}"), |s| {
            let has = |l: &str| s.lines().any(|x| x.trim_end().ends_with(l));
            has(&ready) && has(&dpr)
        });
    }

    /// Another output, of `scale`.
    fn plug(&mut self, scale: f64) {
        let name = format!("HEADLESS-{}", self.outputs.len() + 1);
        // Configured before it exists: it comes up at its scale.
        swaymsg(&self.sway, &format!("output {name} resolution 2560x1600"));
        swaymsg(&self.sway, &format!("output {name} scale {scale}"));
        swaymsg(&self.sway, "create_output");
        self.up(&name, scale);
        self.outputs.push((name, scale));
    }

    /// Takes the last output away.
    fn unplug(&mut self) {
        let (name, _) = self.outputs.pop().unwrap();
        swaymsg(&self.sway, &format!("output {name} unplug"));
        let gone = format!("GONE {name}");
        self.wait(&format!("the panel on {name} gone"), |s| s.contains(&gone));
    }

    fn ipc(&self, func: &str) {
        let mut cmd = Self::program(self.kind);
        let st = self
            .sway
            .env(&mut cmd)
            .arg("-p")
            .arg(self.dir.path())
            .args(["ipc", "call", "p", func])
            .stdout(Stdio::null())
            .status()
            .unwrap();
        assert!(st.success(), "ipc call p {func}");
    }

    /// One more Image in every panel, made in the shown window: `later0` asks for what `sized`
    /// does, `later1` for a size nothing asked for yet.
    fn more(&mut self) {
        self.gen_ += 1;
        self.ipc("more");
        let line = format!("GEN {}", self.gen_);
        self.wait(&line, |s| s.contains(&line));
    }

    /// What every panel holds and shows now: `output name` to the Image's implicit size and
    /// status, and to the colour at its centre.
    fn state(&mut self) -> BTreeMap<String, String> {
        self.tick += 1;
        let tick = self.tick;
        self.ipc("tick");
        let n = self.outputs.len();
        let names: Vec<String> = self.outputs.iter().map(|o| o.0.clone()).collect();
        match self.kind {
            Kind::Ours => {
                let mark = format!("TICK {tick}");
                self.wait("every panel's frame after the tick on screen", |s| {
                    surfaces_shown_after(s, &mark) >= n
                });
            }
            Kind::Quickshell => self.wait("every panel's frame after the tick on screen", |s| {
                names
                    .iter()
                    .all(|o| s.contains(&format!("SHOWN {o} {tick}")))
            }),
        }
        self.ipc("dump");
        self.wait("the dump", |s| {
            names
                .iter()
                .all(|o| s.contains(&format!("DUMPED {o} {tick}")))
        });
        let text = self.log.text();
        let from = text.rfind(&format!("TICK {tick}")).unwrap();
        let mut out = BTreeMap::new();
        for l in text[from..].lines() {
            let Some((_, l)) = l.split_once("IMG ") else {
                continue;
            };
            let l = l.trim_end().trim_end_matches("\u{1b}[0m");
            let mut f = l.splitn(3, ' ');
            let (o, name, rest) = (f.next().unwrap(), f.next().unwrap(), f.next().unwrap());
            out.insert(format!("{o} {name}"), format!("implicit {rest}"));
        }
        for (o, scale) in &self.outputs {
            let (w, px) = self.shot(o);
            for (name, x) in PLACES {
                if !out.contains_key(&format!("{o} {name}")) {
                    continue;
                }
                let at = |v: usize| (v as f64 * scale * factor()) as usize;
                let i = (at(33) * w + at(x + 13)) * 3;
                let c = format!("#{:02x}{:02x}{:02x}", px[i], px[i + 1], px[i + 2]);
                out.entry(format!("{o} {name}"))
                    .and_modify(|v| *v += &format!(", drawn {c}"));
            }
        }
        out
    }

    /// Output `name` as RGB rows with their width, in device pixels.
    fn shot(&self, name: &str) -> (usize, Vec<u8>) {
        let f = self.dir.path().join("shot.ppm");
        let st = self
            .sway
            .env(&mut Command::new("grim"))
            .args(["-t", "ppm", "-o", name])
            .arg(&f)
            .status()
            .expect("grim");
        assert!(st.success(), "grim -o {name}");
        let b = std::fs::read(&f).unwrap();
        // P6\n<w> <h>\n255\n<rgb>
        let mut fields = Vec::new();
        let mut i = 0;
        while fields.len() < 4 {
            while b[i].is_ascii_whitespace() {
                i += 1;
            }
            let s = i;
            while !b[i].is_ascii_whitespace() {
                i += 1;
            }
            fields.push(String::from_utf8_lossy(&b[s..i]).into_owned());
        }
        (fields[1].parse().unwrap(), b[i + 1..].to_vec())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Closing its stdin (test process exit) makes the wrapper shell kill the daemon.
struct Bus(#[allow(dead_code)] Child);

/// In `test`'s own process (see `testenv::run_in_own_process`): the icon theme `r` as the
/// user's, under Qt's own KDE theme, and a private session bus. `None` in the process that ran
/// the test there, or without the tools.
fn setup(test: &str, cache: bool) -> Option<Bus> {
    if !testenv::require_all(&["qs", "sway", "swaymsg", "grim", "dbus-daemon", "gdbus"]) {
        return None;
    }
    if !testenv::in_own_process(test) {
        let home = tempfile::tempdir().unwrap();
        let (config, share) = (home.path().join(".config"), home.path().join("share"));
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(config.join("kdeglobals"), "[Icons]\nTheme=r\n").unwrap();
        write_theme(&share, cache);
        let mut vars = testenv::home_vars(home.path());
        let bus = format!("unix:path={}", home.path().join("bus").display());
        vars.extend([
            ("QT_QPA_PLATFORMTHEME", "kde".into()),
            ("KDE_SESSION_VERSION", "6".into()),
            ("XDG_DATA_DIRS", share.into()),
            ("DBUS_SESSION_BUS_ADDRESS", bus.into()),
        ]);
        testenv::run_in_own_process_env(
            test,
            vars,
            &["XDG_CURRENT_DESKTOP", "KDEHOME", "QS_ICON_THEME"],
        );
        return None;
    }
    let addr = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    let cfg = Path::new(&std::env::var("HOME").unwrap()).join("bus.conf");
    std::fs::write(
        &cfg,
        "<busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen><auth>EXTERNAL</auth>\
         <policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/>\
         <allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>",
    )
    .unwrap();
    let mut c = Command::new("sh")
        .args(["-c", "dbus-daemon --config-file=\"$0\" --address=\"$1\" --nofork --nopidfile --print-address=1 & read _; kill $!"])
        .arg(&cfg)
        .arg(&addr)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("dbus-daemon");
    let mut line = String::new();
    let _ = std::io::BufRead::read_line(
        &mut std::io::BufReader::new(c.stdout.take().unwrap()),
        &mut line,
    );
    assert!(!line.trim().is_empty(), "dbus-daemon did not start");
    Some(Bus(c))
}

/// The entries of `ours` that are not Quickshell's, as lines for the failure.
fn differences(
    what: &str,
    qs: &BTreeMap<String, String>,
    ours: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut wrong = Vec::new();
    for (k, q) in qs {
        let o = ours.get(k).map_or("nothing", |v| v.as_str());
        if !same(q, o) {
            wrong.push(format!("{what}, {k}: Quickshell {q}; ours {o}"));
        }
    }
    for k in ours.keys().filter(|k| !qs.contains_key(*k)) {
        wrong.push(format!("{what}, {k}: Quickshell nothing; ours {}", ours[k]));
    }
    wrong
}

/// The same implicit size and status, and colours within what a blended edge differs by.
fn same(a: &str, b: &str) -> bool {
    let split = |s: &str| {
        let (text, c) = s.split_once(", drawn #").unwrap_or((s, ""));
        let rgb: Vec<i32> = (0..c.len() / 2)
            .map(|i| i32::from_str_radix(&c[2 * i..2 * i + 2], 16).unwrap())
            .collect();
        (text.to_string(), rgb)
    };
    let ((ta, ca), (tb, cb)) = (split(a), split(b));
    ta == tb && ca.len() == cb.len() && ca.iter().zip(&cb).all(|(x, y)| (x - y).abs() <= 3)
}

fn assert_none(wrong: Vec<String>) {
    assert!(
        wrong.is_empty(),
        "{} differ:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

/// Both shells on outputs of `scales`: what differs at the start and for Images made later.
fn fresh(scales: &[f64]) -> Vec<String> {
    let run = |kind| {
        let mut s = Session::start(kind, scales);
        let first = s.state();
        s.more();
        s.more();
        (first, s.state())
    };
    let (qs, ours) = (run(Kind::Quickshell), run(Kind::Ours));
    let mut wrong = differences(&format!("outputs {scales:?} at the start"), &qs.0, &ours.0);
    wrong.extend(differences(
        &format!("outputs {scales:?} with Images made later"),
        &qs.1,
        &ours.1,
    ));
    wrong
}

const ONE: &str = concat!(module_path!(), "::one_output_at_each_scale");

#[test]
fn one_output_at_each_scale() {
    let Some(_bus) = setup(ONE, false) else {
        return;
    };
    let mut wrong = Vec::new();
    for scale in [1.0, 1.5, 2.0, 3.0] {
        wrong.extend(fresh(&[scale]));
    }
    assert_none(wrong);
}

/// `QT_SCALE_FACTOR` of both shells; none unless a test sets one.
static FACTOR: std::sync::Mutex<Option<&str>> = std::sync::Mutex::new(None);

/// [`FACTOR`] as a number: a window is this many times its output's scale.
fn factor() -> f64 {
    FACTOR.lock().unwrap().map_or(1.0, |f| f.parse().unwrap())
}

const FACTORED: &str = concat!(module_path!(), "::an_application_ratio_that_is_not_whole");

/// `QT_SCALE_FACTOR=1.5` makes QGuiApplication::devicePixelRatio() 1.5 on an output of scale
/// 1 and 3 on one of scale 2: an icon asked without a window is `sourceSize` times it.
#[test]
fn an_application_ratio_that_is_not_whole() {
    let Some(_bus) = setup(FACTORED, false) else {
        return;
    };
    *FACTOR.lock().unwrap() = Some("1.5");
    let mut wrong = fresh(&[1.0]);
    wrong.extend(fresh(&[2.0]));
    assert_none(wrong);
}

const TWO: &str = concat!(module_path!(), "::two_outputs_of_different_scales");

#[test]
fn two_outputs_of_different_scales() {
    let Some(_bus) = setup(TWO, false) else {
        return;
    };
    let mut wrong = fresh(&[1.0, 2.0]);
    wrong.extend(fresh(&[1.25, 3.0]));
    assert_none(wrong);
}

const CACHED: &str = concat!(module_path!(), "::a_theme_with_an_icon_cache");

#[test]
fn a_theme_with_an_icon_cache() {
    let Some(_bus) = setup(CACHED, true) else {
        return;
    };
    assert_none(fresh(&[2.0]));
}

const PLUGGED: &str = concat!(
    module_path!(),
    "::a_larger_output_plugged_in_and_out_leaves_the_icons_shown"
);

/// The icons shown stay as they were loaded when an output of a higher scale comes and when it
/// goes; an Image made after asks at the scale of the outputs there are then, unless what it
/// asks for is already loaded.
#[test]
fn a_larger_output_plugged_in_and_out_leaves_the_icons_shown() {
    let Some(_bus) = setup(PLUGGED, false) else {
        return;
    };
    let run = |kind| {
        let mut s = Session::start(kind, &[1.0]);
        let mut states = vec![("one output at 1", s.state())];
        s.plug(2.0);
        states.push(("an output at 2 plugged in", s.state()));
        s.more();
        s.more();
        states.push(("Images made with both outputs", s.state()));
        s.unplug();
        states.push(("the output at 2 unplugged", s.state()));
        s.more();
        states.push(("an Image made after the unplug", s.state()));
        states
    };
    let (qs, ours) = (run(Kind::Quickshell), run(Kind::Ours));
    let mut wrong = Vec::new();
    for ((what, q), (_, o)) in qs.iter().zip(&ours) {
        wrong.extend(differences(what, q, o));
    }
    assert_none(wrong);
}

/// A StatusNotifierItem's pixmaps: width, height, ARGB32 bytes.
type Pixmaps = Vec<(i32, i32, Vec<u8>)>;

/// A StatusNotifierItem with a named icon, a named overlay or a pixmap.
struct Item {
    id: String,
    icon: String,
    overlay: String,
    pixmap: Pixmaps,
}

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl Item {
    #[zbus(property)]
    fn id(&self) -> String {
        self.id.clone()
    }
    #[zbus(property)]
    fn title(&self) -> String {
        self.id.clone()
    }
    #[zbus(property)]
    fn status(&self) -> String {
        "Active".into()
    }
    #[zbus(property)]
    fn category(&self) -> String {
        "ApplicationStatus".into()
    }
    #[zbus(property)]
    fn icon_name(&self) -> String {
        self.icon.clone()
    }
    #[zbus(property)]
    fn icon_pixmap(&self) -> Pixmaps {
        self.pixmap.clone()
    }
    #[zbus(property)]
    fn overlay_icon_name(&self) -> String {
        self.overlay.clone()
    }
    #[zbus(property)]
    fn overlay_icon_pixmap(&self) -> Pixmaps {
        Vec::new()
    }
    #[zbus(property)]
    fn attention_icon_name(&self) -> String {
        String::new()
    }
    #[zbus(property)]
    fn attention_icon_pixmap(&self) -> Pixmaps {
        Vec::new()
    }
    #[zbus(property)]
    fn icon_theme_path(&self) -> String {
        String::new()
    }
    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn tool_tip(&self) -> (String, Pixmaps, String, String) {
        Default::default()
    }
}

/// The tray's Watcher has an owner.
fn watcher_up() {
    const WATCHER: &str = "org.kde.StatusNotifierWatcher";
    let c = zbus::blocking::Connection::session().unwrap();
    let dbus = zbus::blocking::fdo::DBusProxy::new(&c).unwrap();
    // Subscribed before looking, so a claim in between is not missed.
    let mut claims = dbus
        .receive_name_owner_changed_with_args(&[(0, WATCHER)])
        .unwrap();
    if dbus.name_has_owner(WATCHER.try_into().unwrap()).unwrap() {
        return;
    }
    assert!(
        claims.any(|s| s.args().is_ok_and(|a| a.new_owner().is_some())),
        "the bus closed"
    );
}

/// Registers `item` with the tray; the connection keeps it there.
fn register(item: Item) -> zbus::blocking::Connection {
    let conn = zbus::blocking::connection::Builder::session()
        .unwrap()
        .serve_at("/StatusNotifierItem", item)
        .unwrap()
        .build()
        .unwrap();
    let me = conn.unique_name().unwrap().to_string();
    conn.call_method(
        Some("org.kde.StatusNotifierWatcher"),
        "/StatusNotifierWatcher",
        Some("org.kde.StatusNotifierWatcher"),
        "RegisterStatusNotifierItem",
        &(me.as_str(),),
    )
    .unwrap();
    conn
}

const SERVICES: &str = concat!(module_path!(), "::tray_and_notification_icons");

/// A tray item's icon (named, named under an overlay, a pixmap) and a notification's, on an
/// output of scale 2. Quickshell 0.3.2 does not draw the overlay, so those are compared by
/// their size alone.
#[test]
fn tray_and_notification_icons() {
    let Some(_bus) = setup(SERVICES, false) else {
        return;
    };
    let run = |kind| {
        let mut s = Session::start(kind, &[2.0]);
        watcher_up();
        let pixmap = vec![(64, 64, [255u8, 40, 80, 120].repeat(64 * 64))];
        let items = [
            ("named", "x", "", vec![]),
            ("overlaid", "x", "p", vec![]),
            ("pixmap", "", "", pixmap),
        ];
        let _held: Vec<_> = items
            .into_iter()
            .map(|(id, icon, overlay, pixmap)| {
                let c = register(Item {
                    id: id.into(),
                    icon: icon.into(),
                    overlay: overlay.into(),
                    pixmap,
                });
                let line = format!("TRAY HEADLESS-1 {id}");
                s.wait(&line, |s| s.contains(&line));
                c
            })
            .collect();
        let st = Command::new("gdbus")
            .args([
                "call",
                "--session",
                "--dest",
                "org.freedesktop.Notifications",
                "--object-path",
                "/org/freedesktop/Notifications",
                "--method",
                "org.freedesktop.Notifications.Notify",
                "icon_request_scale",
                "0",
                "x",
                "summary",
                "body",
                "[]",
                "{}",
                "0",
            ])
            .stdout(Stdio::null())
            .status()
            .expect("gdbus");
        assert!(st.success(), "gdbus call Notify");
        s.wait("the notification", |s| s.contains("NOTE x"));
        s.state()
    };
    let (qs, ours) = (run(Kind::Quickshell), run(Kind::Ours));
    assert_none(differences("tray and notification", &qs, &ours));
}

const MISSING: &str = concat!(
    module_path!(),
    "::missing_icon_is_the_generic_one_a_found_icon_would_be"
);

/// For a name no theme has impulse-shell draws `application-x-executable` where Quickshell draws a
/// checkerboard (docs/quickshell-differences.md): from the directory the icon asked for
/// would have come from.
#[test]
fn missing_icon_is_the_generic_one_a_found_icon_would_be() {
    let Some(_bus) = setup(MISSING, false) else {
        return;
    };
    let mut wrong = Vec::new();
    {
        // IconImageProvider::missingPixmap: black, two quarters of it #d900d8.
        let mut s = Session::start(Kind::Quickshell, &[1.0]);
        s.state();
        let (w, px) = s.shot("HEADLESS-1");
        let at = |x: usize, y: usize| {
            let i = ((20 + y) * w + 300 + x) * 3;
            [px[i], px[i + 1], px[i + 2]]
        };
        let quarters = [at(6, 6), at(19, 6), at(6, 19), at(19, 19)];
        let (purple, black) = ([0xd9, 0, 0xd8], [0, 0, 0]);
        if quarters != [purple, black, black, purple] && quarters != [black, purple, purple, black]
        {
            wrong.push(format!("Quickshell draws {quarters:?} for a missing icon"));
        }
    }
    for scales in [vec![1.0], vec![2.0], vec![1.0, 2.0]] {
        let mut s = Session::start(Kind::Ours, &scales);
        s.state();
        for (o, scale) in &s.outputs {
            // The generic icon's file is the found one's with another blue.
            let (w, px) = s.shot(o);
            let at = |x: usize| {
                let dev = |v: usize| (v as f64 * scale) as usize;
                let i = (dev(33) * w + dev(x + 13)) * 3;
                [px[i], px[i + 1], px[i + 2]]
            };
            let (found, missing) = (at(20), at(300));
            if [found[0], found[1], 128] != missing {
                wrong.push(format!(
                    "outputs {scales:?}, {o}: found drawn {found:?}, missing drawn {missing:?}"
                ));
            }
        }
    }
    assert_none(wrong);
}

/// A dimension of `sourceSize` that nothing wrote reads as the pixmap's (QQuickImageBase::
/// sourceSize), in the pixmap's pixels: of an Image that set neither, of one that set the
/// width, in a binding, at each scale.
#[test]
fn a_source_size_nothing_wrote_reads_as_the_pixmaps() {
    let test = concat!(
        module_path!(),
        "::a_source_size_nothing_wrote_reads_as_the_pixmaps"
    );
    let Some(_bus) = setup(test, false) else {
        return;
    };
    let mut wrong = Vec::new();
    for scale in [1.0, 2.0, 1.5] {
        let read = |kind| {
            let mut s = Session::start(kind, &[scale]);
            s.state();
            let text = s.log.text();
            let l = text.lines().rfind(|l| l.contains("UNSET HEADLESS-1 "));
            let l = l.unwrap().split_once("UNSET HEADLESS-1 ").unwrap().1;
            // Qt names the type QSize; here it is the QSizeF `Qt.size` makes.
            let l = l.trim_end().trim_end_matches("\u{1b}[0m");
            l.replace("QSizeF(", "QSize(")
        };
        let (qs, ours) = (read(Kind::Quickshell), read(Kind::Ours));
        if qs != ours {
            wrong.push(format!("scale {scale}: Quickshell {qs}; ours {ours}"));
        }
    }
    assert_none(wrong);
}

/// An Image made once its window is there and before the compositor gave the window a scale
/// asks at the scale of the window's output, not at the highest of all outputs.
#[test]
fn an_image_made_after_its_window_and_before_the_windows_first_scale() {
    let test = concat!(
        module_path!(),
        "::an_image_made_after_its_window_and_before_the_windows_first_scale"
    );
    let Some(_bus) = setup(test, false) else {
        return;
    };
    let mut wrong = Vec::new();
    for scales in [[1.0, 2.0], [2.0, 1.0], [1.5, 3.0]] {
        let run = |kind| {
            let mut state = Session::start(kind, &scales).state();
            state.retain(|k, _| k.contains(" early-"));
            state
        };
        let (qs, ours) = (run(Kind::Quickshell), run(Kind::Ours));
        wrong.extend(differences(&format!("outputs {scales:?}"), &qs, &ours));
    }
    assert_none(wrong);
}
