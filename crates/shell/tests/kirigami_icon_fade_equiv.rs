//! A Kirigami.Icon with `animated: true` (the original's FluentIcon) fades from the image it shows into
//! the new one, as Kirigami 6.31 does on Qt 6.12 (Icon::updatePolish, shaders/icon.frag): over
//! Units.longDuration (200 ms) on QEasingCurve::InOutCubic, each pixel `mix(old, new, value)`
//! of the two images, a mask filled with the current colour in both. We drew the new image at
//! once.
//!
//! Compared frame by frame with Qt itself: Qt 6.12 and Kirigami on OpenGL under the test
//! compositor, its animation clock stepped by frame (`QSG_FIXED_ANIMATION_STEP`), every frame
//! grabbed and numbered by the frame counter painted into it. Our clock takes the same step
//! per frame; no wall time is read.
//!
//! Two frames pass in Qt before the fade moves, and they are behaviour: Icon::updatePolish
//! starts the animation at 0 in the frame of the write, and the animation driver's step at
//! the end of that frame does not reach it (QSGThreadedRenderLoop::polishAndSync: polishItems,
//! the sync, then advance, in one call; QPropertyAnimation::start() only queues the animation,
//! QAnimationTimer::registerAnimation, and QAnimationTimer::updateAnimationsTime moves the
//! animations startAnimations has taken). So the frame after the write is drawn at 0 ms as
//! well and the second one after it at one step. We do the same, and no frame more:
//! `our_fade_moves_in_the_second_frame_after_the_write`.
//!
//! Qt may stand for more frames, each of them drawn with the fade running at 0 ms. It is the
//! order of its event queue (Qt 6.12.0). The driver moves the animation once two queued calls
//! have run: QAnimationTimer::startAnimations (queued at qabstractanimation.cpp:662) and
//! then, when no other C++ animation runs, QUnifiedTimer::startTimers (:431). A call queued
//! while queued calls run waits for the next round (QCoreApplicationPrivate::sendPostedEvents,
//! qcoreapplication.cpp:1844 and :1884). The update requests of the frames come through the
//! same queue: QSGThreadedRenderLoop::polishAndSync asks for the next one after every step
//! (qsgthreadedrenderloop.cpp:1717), QWaylandWindow::requestUpdate queues a call that
//! delivers it when no frame callback is outstanding (qwaylandwindow.cpp:1776), and a frame
//! callback queues one as well (:871). Every update request ahead of a queued call is one
//! more frame at 0 ms. Measured with two calls of the oracle's own, queued where Qt queues
//! its two (a build that is not kept; twelve runs at once and sixteen busy loops, 120 fades):
//! 1 to 9 frames were made before the first was delivered and 0 to 7 before the second, and
//! every fade moved within a step of the second; on an idle machine 1 and 0 to 1 (60 fades).
//! Which of the two queued deliveries brought each of those frames was not measured. A GUI
//! thread kept longer than a frame of the output in every step does the same on an idle
//! machine, the frame callback coming while it is kept: kept 5 or 12 ms Qt stood 0 to 3
//! frames, kept 20, 30 or 45 ms up to 27, 26 and 44
//! (`qt_kept_45_ms_in_every_step_stands_before_its_fades`). That is frame scheduling: we do
//! not stand, and nothing compared depends on it:
//!
//! - The steps of the test that come after a write of the source are not made at a frame
//!   number but when Qt's fade after that write has made a given number of steps (`At`), so
//!   a write meant for the middle of a fade is in the middle of it however long Qt stood.
//!   The scene reads the time of the fade at every frame from the icon's own
//!   QPropertyAnimation, which QML cannot reach: Qt is run by tools/oracle/icon_fade_oracle.py,
//!   which only reads it. (What a scene can see of its frames in QML, a grabToImage result,
//!   comes through that same queue, up to 8 frames late on busy processors.)
//! - `qt_fades` takes from the times Qt's scene read where each of its fades began, and holds
//!   Qt's pictures to it; our steps are made at the frames Qt made them, our clock stands
//!   still for the frames Qt stood (`our_frames`), and every frame Qt drew is compared.
//!
//! On busy processors Qt at times draws no frame for a frame number: its animations make a
//! step with no frame prepared since the step before (no QQuickWindow::afterAnimating between
//! the two), so the frame counter moves twice before one picture. Measured with twelve runs
//! at once and sixteen busy loops: every number without a picture was such a step (19 of 19,
//! in 17 of 288 runs), the time of the fade did not move on it, and it came one to six frames
//! after a write of the source (once after a write of the colour alone, which starts the
//! icon's animation as well: Kirigami's Icon::updatePolish), never on the frame of the write,
//! the frame the fade began in or the one before it. The stack of that step was taken three
//! times, in a build that prints it, and was the same: a queued
//! QAnimationTimer::startAnimations, whose QUnifiedTimer::maybeUpdateAnimationsToCurrentTime
//! steps every running animation when more than 50 ms passed since the last step
//! (qabstractanimation.cpp, Qt 6.12.0). 13 of the 19 came more than 50 ms after the step
//! before them by the scene's wall clock; for the other 6 (0 or 1 ms) the condition is not
//! established. Such a number has no picture of Qt's to compare with: it is printed and not
//! compared, and it fails the test where it would place a fade (`qt_fades`).

use qml::runtime::Runtime;
use qml::value::Value;
use qsg::raster::Canvas;
use shell::scene::{self, Target};
use std::collections::BTreeMap;
use std::path::Path;

const W: usize = 224;
const H: usize = 32;
/// QSG_FIXED_ANIMATION_STEP: the animation driver's step per frame, in ms.
const STEP: f64 = 16.0;
/// The first frame compared: Qt's first ones are its window coming up.
const FIRST: usize = 30;
/// The PySide6 of the testenv image (tools/testenv/Containerfile).
const PYTHON: &str = "/opt/qv4-venv/bin/python";

/// `a` and `b` fill the left and the right half of their square; `c` and `d` overlap in the
/// middle third, in two colours.
fn write_icons(dir: &Path) {
    let icons = [
        ("a", 0, 16, "#ff0000"),
        ("b", 16, 16, "#ff0000"),
        ("c", 0, 21, "#ff0000"),
        ("d", 11, 21, "#0000ff"),
    ];
    for (name, x, width, fill) in icons {
        let svg = format!(
            "<svg width=\"32\" height=\"32\" viewBox=\"0 0 32 32\" \
             xmlns=\"http://www.w3.org/2000/svg\">\
             <rect x=\"{x}\" y=\"0\" width=\"{width}\" height=\"32\" fill=\"{fill}\"/></svg>\n"
        );
        std::fs::write(dir.join(format!("{name}.svg")), svg).unwrap();
    }
}

/// The icons, one per 32 px slot, and the pixels of each slot that are compared (x in the
/// slot, at mid height).
const SLOTS: [(&str, &[usize]); 6] = [
    ("animated", &[6, 18]),
    ("animated mask", &[6, 18]),
    ("not animated", &[6, 18]),
    ("animated, under an item that hides", &[6, 18]),
    ("animated, resized", &[6, 18]),
    ("animated, overlapping images", &[6, 16, 26]),
];

/// The scene: `root` holds the icons and a square that shows the frame counter `fc`.
fn body(dir: &Path) -> String {
    let dir = dir.display();
    format!(
        r##"Item {{
    id: root
    width: {W}; height: {H}
    property string shown: "a"
    property string file: shown === "" ? "" : "{dir}/" + shown + ".svg"
    property color tint: "#00ff00"
    property bool vis: true
    property real sz: 32
    property int fc: 0
    Rectangle {{ anchors.fill: parent; color: "#000000" }}
    Kirigami.Icon {{
        objectName: "first"
        width: 32; height: 32; roundToIconSize: false
        source: root.file; animated: true
    }}
    Kirigami.Icon {{
        x: 32; width: 32; height: 32; roundToIconSize: false
        source: root.file; animated: true; isMask: true; color: root.tint
    }}
    Kirigami.Icon {{
        x: 64; width: 32; height: 32; roundToIconSize: false
        source: root.file
    }}
    Item {{
        x: 96; width: 32; height: 32; visible: root.vis
        Kirigami.Icon {{
            width: 32; height: 32; roundToIconSize: false
            source: root.file; animated: true
        }}
    }}
    Kirigami.Icon {{
        x: 128; width: root.sz; height: root.sz; roundToIconSize: false
        source: root.file; animated: true
    }}
    Kirigami.Icon {{
        x: 160; width: 32; height: 32; roundToIconSize: false
        source: "{dir}/" + (root.shown === "b" ? "d" : "c") + ".svg"; animated: true
    }}
    Rectangle {{
        x: 192; width: 32; height: 32
        color: Qt.rgba(Math.floor(root.fc / 256) / 255, (root.fc % 256) / 255, 0, 1)
    }}
"##
    )
}

/// When a step of the test is made.
#[derive(Clone, Copy)]
enum At {
    /// At this frame.
    Frame(usize),
    /// When Qt's fade of the first icon after the last write of the source has made this many
    /// steps: at the first one the fade is at 16 ms.
    FadeStep(usize),
}
use At::{FadeStep, Frame};

/// Root properties written in one step, as JavaScript literals.
type Step = (At, &'static [(&'static str, &'static str)]);

/// The steps, in order. With a Qt that does not stand they are made at the frames in the
/// comments.
const STEPS: &[Step] = &[
    // A new source. (40)
    (Frame(40), &[("shown", "\"b\"")]),
    // Another one while the fade runs: at 64 of its 200 ms. (70, 76)
    (FadeStep(28), &[("shown", "\"a\"")]),
    (FadeStep(4), &[("shown", "\"b\"")]),
    // A new colour alone, and one while a fade runs. (100, 110, 114)
    (FadeStep(22), &[("tint", "\"#ffff00\"")]),
    (FadeStep(32), &[("shown", "\"a\"")]),
    (FadeStep(2), &[("tint", "\"#00ffff\"")]),
    // To no image and back. (140, 165)
    (FadeStep(28), &[("shown", "\"\"")]),
    (FadeStep(23), &[("shown", "\"b\"")]),
    // A new size in the same turn. (190)
    (FadeStep(23), &[("shown", "\"a\""), ("sz", "24")]),
    // Hidden and shown again before the source changes; then the next change. (215, 219, 225,
    // 250)
    (FadeStep(23), &[("vis", "false")]),
    (FadeStep(27), &[("vis", "true")]),
    (FadeStep(33), &[("shown", "\"b\"")]),
    (FadeStep(23), &[("shown", "\"a\"")]),
    // The source changes while hidden, shown again within the fade. (275, 277, 280)
    (FadeStep(23), &[("vis", "false")]),
    (FadeStep(25), &[("shown", "\"b\"")]),
    (FadeStep(1), &[("vis", "true")]),
];
/// The last frame: so many steps of the last fade. (300)
const END: At = FadeStep(21);
/// Qt gives up on a fade that has not moved by this frame.
const GIVE_UP: usize = 2000;

fn writes_source(writes: &[(&str, &str)]) -> bool {
    writes.iter().any(|(k, _)| *k == "shown")
}

/// The compared pixels of one frame, in `SLOTS` order.
type Probes = Vec<[u8; 3]>;

fn probes(px: impl Fn(usize, usize) -> [u8; 3]) -> Probes {
    (SLOTS.iter().enumerate())
        .flat_map(|(n, (_, xs))| xs.iter().map(move |x| (n * 32 + x, H / 2)))
        .map(|(x, y)| px(x, y))
        .collect()
}

/// How long Qt's scene keeps its GUI thread inside a step of the animations
/// (FrameAnimation.onTriggered), reading the wall clock in a loop.
#[derive(Clone, Copy)]
enum Kept {
    No,
    /// So many ms in every step.
    EveryStep(u32),
    /// 70 ms in the step at which the scene first reads a time above 0 of a fade, at every
    /// fade. Before it keeps the thread the scene writes the colour of one more animated
    /// Kirigami.Icon, under the black of `root`, and has it polished at once
    /// (QQuickItem::ensurePolished), which starts that icon's animation (Icon::updatePolish).
    /// The queued QAnimationTimer::startAnimations of it then runs more than 50 ms after the
    /// last step and makes one more (see the module comment): Qt draws no frame for the
    /// number of the step that was kept, the one the fade began in.
    WhereTheFadeBegins,
}

/// One run of Qt.
struct QtRun {
    /// The compared frames as Qt drew them, by frame counter: `FIRST..=last`.
    frames: BTreeMap<usize, Probes>,
    /// The frame at which Qt made each of `STEPS`.
    steps: Vec<usize>,
    /// The frame of `END`.
    last: usize,
    /// The time of the first icon's fade Qt's scene read at each step `FIRST..=last`, in ms
    /// (-1: no fade runs): what the frame before that step showed.
    times: BTreeMap<usize, i64>,
    /// The frame numbers Qt drew no frame for: its animations made two steps between two
    /// frames (see the module comment), so the frame after them shows the next number.
    undrawn: Vec<usize>,
}

/// Runs the scene under Qt; `None` (reported) without the oracle's Python.
fn qt_run(dir: &Path, kept: Kept) -> Option<QtRun> {
    if !Path::new(PYTHON).exists() {
        testenv::equiv_missing(format_args!("{PYTHON} (PySide6)"));
        return None;
    }
    if !testenv::require("sway") {
        return None;
    }
    let sway = testenv::Sway::start();
    let plan: String = (STEPS.iter().map(|(at, writes)| (*at, *writes)))
        .chain([(END, &[][..])])
        .enumerate()
        .map(|(i, (at, writes))| {
            let (frame, steps) = match at {
                Frame(n) => (n, 0),
                FadeStep(k) => (0, k),
            };
            let run: String = if i == STEPS.len() {
                "win.last = root.fc; ".into()
            } else {
                (writes.iter())
                    .map(|(k, v)| format!("root.{k} = {v}; "))
                    .collect()
            };
            format!(
                "        {{ frame: {frame}, steps: {steps}, source: {}, run: () => {{ {run}}} }},\n",
                writes_source(writes)
            )
        })
        .collect();
    let (every, begins) = match kept {
        Kept::No => (0, false),
        Kept::EveryStep(ms) => (ms, false),
        Kept::WhereTheFadeBegins => (0, true),
    };
    let helper = if begins {
        format!(
            "Kirigami.Icon {{\n        id: other\n        \
             width: 32; height: 32; roundToIconSize: false\n        \
             source: \"{}/a.svg\"; animated: true; isMask: true\n        \
             color: win.kick ? \"#010101\" : \"#020202\"\n    }}",
            dir.display()
        )
    } else {
        String::new()
    };
    let source = format!(
        r##"import QtQuick
import org.kde.kirigami as Kirigami
Window {{
    id: win
    width: {W}; height: {H}; visible: true; color: "#000000"
    // The animation clock, in ms.
    property real clock: 0
    property var clocks: []
    NumberAnimation on clock {{ from: 0; to: 600000; duration: 600000 }}
    // The steps of the test, in order: each is made at `frame` or, with `steps`, when the
    // fade after the last write of the source has made that many steps.
    property var plan: [
{plan}    ]
    property int next: 0
    // The steps the fade after the last write of the source has made: none until it moves.
    property int steps: 0
    // The last frame, once the plan is through.
    property int last: -1
    // The frames the render loop has prepared (QQuickWindow::afterAnimating, in polishAndSync):
    // one before every step of the animations it makes.
    property int prepared: 0
    onAfterAnimating: prepared++
    // Per step from the one before the first compared frame: the time of the fade, and
    // `prepared`.
    property var times: []
    property var frames: []
    // `Kept::WhereTheFadeBegins`: the colour of its icon.
    property bool kick: false
    {helper}
    {body}
        FrameAnimation {{
            running: true
            onTriggered: {{
                root.fc++;
                // Of the first icon's fade, in ms, as the frame before this one showed it.
                const time = fade.time();
                if (root.fc >= {FIRST} - 1 && (win.last < 0 || root.fc <= win.last)) {{
                    win.times.push(time);
                    win.frames.push(win.prepared);
                }}
                const keep = {every} > 0 ? {every} : {begins} && time > 0 && win.steps == 0 ? 70 : 0;
                if ({begins} && keep > 0) {{
                    win.kick = !win.kick;
                    other.ensurePolished();
                }}
                for (const from = Date.now(); Date.now() - from < keep;) {{}}
                if (win.steps > 0)
                    win.steps++;
                else if (time > 0)
                    win.steps = 1;
                while (win.next < win.plan.length) {{
                    const step = win.plan[win.next];
                    if (step.steps > 0 ? win.steps != step.steps : root.fc != step.frame)
                        break;
                    step.run();
                    console.log("STEP", win.next, root.fc);
                    if (step.source)
                        win.steps = 0;
                    win.next++;
                }}
                if (win.last < 0 && root.fc == {GIVE_UP}) {{
                    console.log("STUCK", win.next);
                    Qt.quit();
                }}
                if (root.fc < {FIRST} || (win.last >= 0 && root.fc > win.last))
                    return;
                if (root.fc == {FIRST})
                    console.log("API", root.GraphicsInfo.api, Screen.devicePixelRatio,
                                "LONG", Kirigami.Units.longDuration);
                const n = root.fc;
                win.clocks.push(Math.round(win.clock));
                root.grabToImage(r => {{
                    r.saveToFile("{dir}/f" + n + ".ppm");
                    if (n == win.last) {{
                        console.log("CLOCKS", win.clocks.join(" "));
                        console.log("TIMES", win.times.join(" "));
                        console.log("PREPARED", win.frames.join(" "));
                        Qt.quit();
                    }}
                }});
            }}
        }}
    }}
}}
"##,
        body = body(dir),
        dir = dir.display()
    );
    let file = dir.join("icons.qml");
    std::fs::write(&file, source).unwrap();
    let oracle =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/oracle/icon_fade_oracle.py");
    let mut cmd = std::process::Command::new(PYTHON);
    cmd.arg(oracle)
        .arg(&file)
        .env("QT_QPA_PLATFORM", "wayland-egl")
        .env("LIBGL_ALWAYS_SOFTWARE", "1")
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .env("QSG_FIXED_ANIMATION_STEP", "1")
        // A window whose frame the compositor has not answered for 100 ms is hidden to Qt,
        // which then moves its animations by a timer, with no frame drawn
        // (QSGThreadedRenderLoop::startOrStopAnimationTimer): frames of the counter are never
        // seen. Two places wait that long. QWaylandGLContext::swapBuffers does
        // (waitForFrameSync(100)) unless Mesa is left to wait in eglSwapBuffers, which has no
        // limit; QWaylandWindow's timer is the other (mFrameCallbackTimeout; 0 is no limit).
        .env("QT_WAYLAND_FORCE_NONBLOCKING_SWAP_SUPPORT", "0")
        .env("QT_WAYLAND_FRAME_CALLBACK_TIMEOUT", "0")
        // The Qt of the system, which Kirigami is built for and qml6 runs on, and not the one
        // PySide6's wheel brings: its modules name that one by an rpath, so the system's
        // libraries are loaded before them, and its qt.conf points at the wheel's plugins.
        // The oracle prints what it has loaded.
        .env(
            "LD_PRELOAD",
            ["Core", "Gui", "Network", "Qml"]
                .map(|l| format!("/usr/lib/libQt6{l}.so.6"))
                .join(":"),
        )
        .env("LD_LIBRARY_PATH", "/usr/lib")
        .env("PYSIDE_DISABLE_INTERNAL_QT_CONF", "1")
        .env("XDG_CACHE_HOME", dir);
    let out = sway.env(&mut cmd).output().unwrap();
    let log = String::from_utf8_lossy(&out.stderr);
    // QSGRendererInterface::OpenGL at scale 1 (Kirigami does not fade on the software
    // backend), and the duration of the fade.
    assert!(log.contains("API 3 1 LONG 200"), "Qt printed:\n{log}");
    // The animation the oracle reads is the icon's fade.
    assert!(
        log.contains("FADE QPropertyAnimation 200"),
        "Qt printed:\n{log}"
    );
    // Every Qt and Kirigami library the oracle has loaded is the system's.
    let libraries = (log.lines().find_map(|l| l.strip_prefix("LIBRARIES ")))
        .unwrap_or_else(|| panic!("Qt printed:\n{log}"));
    let foreign: Vec<&str> = (libraries.split(' '))
        .filter(|l| !l.starts_with("/usr/lib/"))
        .collect();
    assert!(
        libraries.contains("/libQt6Quick.so")
            && libraries.contains("/libKirigami")
            && foreign.is_empty(),
        "the oracle runs on libraries that are not the system's: {foreign:?} of {libraries}"
    );
    let mut steps: Vec<usize> = (log.lines())
        .filter_map(|l| l.split_once("STEP ")?.1.split_once(' ')?.1.parse().ok())
        .collect();
    if let Some(at) = log
        .lines()
        .find_map(|l| l.split_once("STUCK ").map(|s| s.1))
    {
        panic!(
            "Qt's fade has not moved by frame {GIVE_UP}: it made the steps at {steps:?} and waits with step {at}"
        );
    }
    assert_eq!(steps.len(), STEPS.len() + 1, "Qt printed:\n{log}");
    let last = steps.pop().unwrap();
    let clocks: Vec<f64> = (log.lines())
        .find_map(|l| l.split_once("CLOCKS ").map(|c| c.1))
        .unwrap_or_else(|| panic!("Qt did not finish; it printed:\n{log}"))
        .split(' ')
        .map(|c| c.parse().unwrap())
        .collect();
    assert!(
        clocks.len() == last - FIRST + 1 && clocks.windows(2).all(|c| c[1] - c[0] == STEP),
        "Qt's animation clock did not move {STEP} ms per frame: {clocks:?}"
    );
    let numbers = |name: &str| -> Vec<i64> {
        (log.lines())
            .find_map(|l| l.split_once(name).map(|c| c.1))
            .unwrap_or_else(|| panic!("Qt did not finish; it printed:\n{log}"))
            .split(' ')
            .map(|c| c.parse().unwrap())
            .collect()
    };
    // Both from the step before `FIRST`.
    let (read, prepared) = (numbers("TIMES "), numbers("PREPARED "));
    assert!(
        read.len() == last - FIRST + 2 && prepared.len() == read.len(),
        "Qt's steps {FIRST}..={last}: {} times, {} frame counts",
        read.len(),
        prepared.len()
    );
    let times: BTreeMap<usize, i64> = (FIRST..=last).zip(read[1..].iter().copied()).collect();
    // Step n came with no frame prepared since step n - 1: no frame shows n - 1.
    let undrawn: Vec<usize> = (FIRST..=last)
        .filter(|n| prepared[n - FIRST + 1] == prepared[n - FIRST])
        .map(|n| n - 1)
        .collect();
    let mut frames = BTreeMap::new();
    for n in FIRST..=last {
        let ppm = std::fs::read(dir.join(format!("f{n}.ppm")))
            .unwrap_or_else(|e| panic!("no grab {n} ({e}); Qt printed:\n{log}"));
        // "P6\n<width> <height>\n255\n", then rows of RGB.
        let header = ppm.splitn(4, |&b| b == b'\n').take(3).map(|l| l.len() + 1);
        let data = &ppm[header.sum::<usize>()..];
        assert_eq!(data.len(), W * H * 3, "grab {n} is not {W}x{H}");
        let px =
            |x: usize, y: usize| -> [u8; 3] { data[(y * W + x) * 3..][..3].try_into().unwrap() };
        // The frame the grab shows is the one its counter square says.
        let [hi, lo, _] = px(208, H / 2);
        frames.insert(usize::from(hi) * 256 + usize::from(lo), probes(px));
    }
    // A number without a picture is one Qt made a second step on, and no other.
    let missing: Vec<usize> = (FIRST..last).filter(|n| !frames.contains_key(n)).collect();
    let stepped: Vec<usize> = (undrawn.iter().copied())
        .filter(|n| (FIRST..last).contains(n))
        .collect();
    assert_eq!(
        missing, stepped,
        "the frames no grab shows, and the frames Qt stepped over; Qt printed:\n{log}"
    );
    Some(QtRun {
        frames,
        steps,
        last,
        times,
        undrawn,
    })
}

/// The frames Qt stood, past the two it needs, before each of its fades began, by the frame
/// of the write of the source. A fade has begun at the first step at which Qt's scene reads a
/// time of the fade above 0: the frame of that step is the third after the write when Qt did
/// not stand. The pictures have to say the same: it is the first frame in which the first
/// icon, which fades at every such write, is seen to leave the old image by 3 of 255 or more,
/// the fade at two steps (4 of 255; at one step it is 1, less than two renderers may differ
/// by). The frame before it, at one step, is compared with ours at one step like every other
/// frame. A fade that has not begun by the next write of the source, or by the last frame,
/// fails the test; so does a step Qt made at another frame than the one the fade gives for
/// it, and a frame Qt did not draw where the fade is placed: the frame of the write, the one
/// the fade begins in, or the one before that.
fn qt_fades(qt: &QtRun) -> BTreeMap<usize, usize> {
    let mut stood = BTreeMap::new();
    // The frame of the last write of the source, and the one its fade began in.
    let mut fade: Option<(usize, usize)> = None;
    for (i, ((at, writes), &frame)) in STEPS.iter().zip(&qt.steps).enumerate() {
        if let (FadeStep(k), Some((wrote, began))) = (at, fade) {
            assert_eq!(
                frame,
                began + k - 1,
                "Qt made step {i} at frame {frame}; the fade after the write at frame {wrote} \
                 began in frame {began}, which puts {k} steps at {}",
                began + k - 1
            );
        }
        if !writes_source(writes) {
            continue;
        }
        let next = (STEPS.iter().zip(&qt.steps).skip(i + 1))
            .find(|((_, writes), _)| writes_source(writes))
            .map_or(qt.last, |(_, &frame)| frame);
        let began = (frame + 1..next)
            .find(|n| qt.times[n] > 0)
            .unwrap_or_else(|| {
                panic!("Qt's fade after the write at frame {frame} has not begun by frame {next}")
            });
        assert!(
            began >= frame + 3,
            "Qt's fade after the write at frame {frame} moves at frame {began}, before two steps"
        );
        for (placed, what) in [
            (frame, "the frame of the write"),
            (began - 1, "the frame before the fade began"),
            (began, "the frame the fade began in"),
        ] {
            assert!(
                !qt.undrawn.contains(&placed),
                "Qt drew no frame {placed}, {what}: the write at frame {frame}, its fade from \
                 frame {began}"
            );
        }
        let left = |n: &usize| {
            let (was, is) = (&qt.frames[&frame], &qt.frames[n]);
            (0..2).any(|k| (0..3).any(|c| was[k][c].abs_diff(is[k][c]) >= 3))
        };
        let seen = (frame..next).filter(|n| !qt.undrawn.contains(n)).find(left);
        assert_eq!(
            seen,
            Some(began),
            "the first frame that shows Qt's fade after the write at frame {frame}; Qt read \
             its time above 0 first at frame {began}"
        );
        stood.insert(frame, began - (frame + 3));
        fade = Some((frame, began));
    }
    if let (FadeStep(k), Some((wrote, began))) = (END, fade) {
        assert_eq!(
            qt.last,
            began + k - 1,
            "Qt's last frame; the fade after the write at frame {wrote} began in frame {began}"
        );
    }
    stood
}

/// What is printed of the frame numbers Qt drew no frame for: there is no picture of Qt's
/// to compare ours with.
fn undrawn_line(qt: &QtRun) -> Option<String> {
    (!qt.undrawn.is_empty()).then(|| format!("Qt drew no frame for {:?}: not compared", qt.undrawn))
}

fn value(js: &str) -> Value {
    match js.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        Some(s) => Value::str(s),
        None => match js.parse::<f64>() {
            Ok(n) => Value::Num(n),
            Err(_) => Value::Bool(js == "true"),
        },
    }
}

/// The same frames under our engine, drawn as the shell draws a window: each frame moves the
/// animation clock one step, and is drawn only when a write touched something, with the
/// recorded draws of what was touched dropped first. The steps are made at the frames Qt made
/// them; where Qt stood (`qt_fades`), our clock stands still for as many frames once our fade
/// began.
fn our_frames(
    dir: &Path,
    qt: &QtRun,
    stood: &BTreeMap<usize, usize>,
    on_gpu: bool,
) -> BTreeMap<usize, Probes> {
    let mut rt = Runtime::new(None);
    let source = format!(
        "import QtQuick\nimport org.kde.kirigami as Kirigami\n{}}}\n",
        body(dir)
    );
    let root = rt.load_str(&source).expect("scene loads");
    let mut fonts = scene::Fonts::default();
    let mut gl = on_gpu.then(|| {
        let (gpu, _) =
            shell::gpu::Gpu::headless("surfaceless").expect("surfaceless EGL/GLES2 context (Mesa)");
        let surface = gpu.pbuffer(W as i32, H as i32).expect("pbuffer");
        (gpu, surface)
    });
    let mut frames = BTreeMap::new();
    let mut last: Option<Probes> = None;
    let mut now = 0.0;
    for n in 1..=qt.last {
        if !(stood.iter()).any(|(at, frames)| (at + 2..at + 2 + frames).contains(&n)) {
            now += STEP;
        }
        {
            let mut w = rt.world.borrow_mut();
            let made = STEPS.iter().zip(&qt.steps).filter(|(_, at)| **at == n);
            for ((_, writes), _) in made {
                for (k, v) in *writes {
                    w.write(root, k, value(v));
                }
            }
        }
        rt.tick(now);
        let (touched, moved) = {
            let mut w = rt.world.borrow_mut();
            (std::mem::take(&mut w.touched), std::mem::take(&mut w.moved))
        };
        if last.is_none() || !touched.is_empty() || !moved.is_empty() {
            {
                let w = rt.world.borrow();
                scene::invalidate_moved(&w, &moved);
                scene::invalidate_layers(&w, &touched);
            }
            // The first frame twice: the second one replays what the first recorded.
            let mut c = Canvas::new(W, H, [0, 0, 0, 0]);
            for _ in 0..if last.is_none() { 2 } else { 1 } {
                scene::layout_window(&mut rt, &mut fonts, root, [W as f64, H as f64]);
                rt.settle();
                match &mut gl {
                    None => {
                        c = Canvas::new(W, H, [0, 0, 0, 0]);
                        scene::paint_window(&mut rt, &mut fonts, root, &mut Target::Cpu(&mut c));
                    }
                    Some((gpu, surface)) => {
                        let mut fr = shell::gpu::Frame::new(W, H);
                        scene::paint_window(&mut rt, &mut fonts, root, &mut Target::Gpu(&mut fr));
                        surface.reswap();
                        gpu.render(surface, &mut fr, || {});
                        c = gpu.read_surface(surface);
                    }
                }
            }
            last = Some(probes(|x, y| {
                let p = c.px[y * W + x];
                [p[0], p[1], p[2]]
            }));
        }
        if n >= FIRST {
            frames.insert(n, last.clone().unwrap());
        }
    }
    frames
}

/// QEasingCurve::InOutCubic.
fn in_out_cubic(t: f64) -> f64 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (2.0 - 2.0 * t).powi(3) / 2.0
    }
}

fn near(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 2)
}

/// Our fade takes the two frames Qt's needs and no frame more, with nothing of Qt's frame
/// scheduling in it: a source written before frame N is drawn in N and N + 1 as the old image,
/// to the pixel, and the fade (the icon's `ICON_MIX`) is at InOutCubic of one step in N + 2,
/// of two in N + 3, and so on to the new image alone after 200 ms.
#[test]
fn our_fade_moves_in_the_second_frame_after_the_write() {
    const N: usize = 5;
    let dir = tempfile::tempdir().unwrap();
    write_icons(dir.path());
    let mut rt = Runtime::new(None);
    let source = format!(
        r##"import QtQuick
import org.kde.kirigami as Kirigami
Item {{
    width: 32; height: 32
    property string shown: "a"
    Rectangle {{ anchors.fill: parent; color: "#000000" }}
    Kirigami.Icon {{
        width: 32; height: 32; roundToIconSize: false
        source: "{dir}/" + parent.shown + ".svg"; animated: true
    }}
}}
"##,
        dir = dir.path().display()
    );
    let root = rt.load_str(&source).expect("scene loads");
    let mut fonts = scene::Fonts::default();
    // The fade's 200 ms end in the 13th step: 208 ms.
    let steps = (200.0 / STEP).ceil() as usize;
    for n in 1..=N + 1 + steps + 2 {
        if n == N {
            rt.world.borrow_mut().write(root, "shown", Value::str("b"));
        }
        rt.tick(n as f64 * STEP);
        {
            let mut w = rt.world.borrow_mut();
            let (touched, moved) = (std::mem::take(&mut w.touched), std::mem::take(&mut w.moved));
            scene::invalidate_moved(&w, &moved);
            scene::invalidate_layers(&w, &touched);
        }
        let mut c = Canvas::new(32, 32, [0, 0, 0, 0]);
        scene::layout_window(&mut rt, &mut fonts, root, [32.0, 32.0]);
        rt.settle();
        scene::paint_window(&mut rt, &mut fonts, root, &mut Target::Cpu(&mut c));
        // The left half is the old image's, the right half the new one's.
        let (old, new) = (c.px[16 * 32 + 6], c.px[16 * 32 + 18]);
        let mix = {
            let mut w = rt.world.borrow_mut();
            let icon = *w.icons.keys().next().expect("the icon was painted");
            match w.read(icon, qml::runtime::ICON_MIX) {
                Some(Value::Num(m)) => Some(m),
                _ => None,
            }
        };
        // Frames since the write; the fade's clock starts in the first one.
        let k = n.checked_sub(N);
        let want = match k {
            None => None,
            Some(k) if k > steps => None,
            Some(k) => Some(in_out_cubic(k.saturating_sub(1) as f64 * STEP / 200.0)),
        };
        match (mix, want) {
            (None, None) => {}
            (Some(m), Some(want)) if (m - want).abs() < 1e-12 => {}
            _ => panic!("frame {n} ({k:?} after the write): the fade is at {mix:?}, not {want:?}"),
        }
        let (red, none) = ([255, 0, 0, 255], [0, 0, 0, 255]);
        match k {
            None | Some(0 | 1) => assert_eq!((old, new), (red, none), "frame {n}: the old image"),
            Some(k) if k > steps => {
                assert_eq!((old, new), (none, red), "frame {n}: the new image")
            }
            Some(k) => {
                let m = want.unwrap();
                let (o, w) = ((255.0 * (1.0 - m)).round() as u8, (255.0 * m).round() as u8);
                assert_eq!(
                    (old, new),
                    ([o, 0, 0, 255], [w, 0, 0, 255]),
                    "frame {n} ({k} after the write): the two images at {m}"
                );
            }
        }
    }
}

/// A run of a Qt that never stands, with no frame drawn for `undrawn`: the steps at the frames
/// in the comments of `STEPS`, the time of the fade 0 for two steps after each write of the
/// source and 16 ms more with every step from the third, and the first icon further from the
/// image of the write with every step of the fade: by 1 of 255 at one step, by 4 at two.
fn nominal_run(undrawn: &[usize]) -> QtRun {
    let steps = vec![
        40, 70, 76, 100, 110, 114, 140, 165, 190, 215, 219, 225, 250, 275, 277, 280,
    ];
    let last = 300;
    let sources: Vec<usize> = (STEPS.iter().zip(&steps))
        .filter(|((_, writes), _)| writes_source(writes))
        .map(|(_, &frame)| frame)
        .collect();
    let (mut times, mut frames) = (BTreeMap::new(), BTreeMap::new());
    for n in FIRST..=last {
        // Steps of the fade the step n has read, and those the frame n shows.
        let wrote = sources.iter().rev().find(|&&w| w < n);
        let read = wrote.map_or(0, |w| (n - w).saturating_sub(2).min(13));
        times.insert(
            n,
            match (wrote, read) {
                (None, _) | (_, 13) => -1,
                (_, k) => 16 * k as i64,
            },
        );
        let shown = (sources.iter().rev().find(|&&w| w <= n))
            .map_or(0, |w| (n - w).saturating_sub(1).min(13));
        if !undrawn.contains(&n) {
            let mut picture = vec![[0, 0, 0]; SLOTS.iter().map(|(_, xs)| xs.len()).sum()];
            // One step is 1 of 255, as the fade is at 16 ms; two are 4.
            let shade = if shown == 1 { 1 } else { 4 * shown as u8 };
            picture[0] = [255 - shade, 0, 0];
            frames.insert(n, picture);
        }
    }
    QtRun {
        frames,
        steps,
        last,
        times,
        undrawn: undrawn.to_vec(),
    }
}

/// The run `qt_fades` is tried on below is one it takes: no fade of it stands.
#[test]
fn a_qt_that_does_not_stand_has_every_fade_in_the_third_frame() {
    let run = nominal_run(&[]);
    let stood = qt_fades(&run);
    assert_eq!(stood.len(), 10);
    assert!(stood.values().all(|&frames| frames == 0), "{stood:?}");
    assert_eq!(undrawn_line(&run), None);
}

/// A frame Qt did not draw between the write and the frame before the fade is printed and
/// passed over.
#[test]
fn a_frame_qt_did_not_draw_after_the_write_is_printed_and_not_compared() {
    let run = nominal_run(&[41]);
    assert_eq!(qt_fades(&run)[&40], 0);
    assert_eq!(
        undrawn_line(&run).as_deref(),
        Some("Qt drew no frame for [41]: not compared")
    );
}

#[test]
#[should_panic(expected = "Qt drew no frame 40, the frame of the write")]
fn a_frame_qt_did_not_draw_fails_on_the_frame_of_the_write() {
    qt_fades(&nominal_run(&[40]));
}

#[test]
#[should_panic(expected = "Qt drew no frame 42, the frame before the fade began")]
fn a_frame_qt_did_not_draw_fails_on_the_frame_before_the_fade() {
    qt_fades(&nominal_run(&[42]));
}

#[test]
#[should_panic(expected = "Qt drew no frame 43, the frame the fade began in")]
fn a_frame_qt_did_not_draw_fails_on_the_frame_the_fade_began_in() {
    qt_fades(&nominal_run(&[43]));
}

/// The frames Qt stood before each of its fades by the times its scene read alone: no
/// picture is looked at, and nothing `qt_fades` holds Qt to is asked.
fn stood_by_times(qt: &QtRun) -> Vec<usize> {
    (STEPS.iter().zip(&qt.steps))
        .filter(|((_, writes), _)| writes_source(writes))
        .map(|(_, &frame)| {
            let began = (frame + 1..=qt.last).find(|n| qt.times[n] > 0);
            began.expect("a fade after every write of the source") - (frame + 3)
        })
        .collect()
}

/// Qt whose GUI thread is kept 45 ms in every step, longer than a frame of the output, stands
/// before its fades on an idle machine as it does on busy processors (see the module
/// comment). In 9 runs on an idle machine the longest it stood was 20 to 44 frames, the
/// shortest 1 to 7; in 12 runs at once with sixteen busy loops 6 to 13 and 0 to 2. We stand
/// none (`our_fade_moves_in_the_second_frame_after_the_write`). No picture of this run is
/// compared: with a queue that long the QAnimationTimer::startAnimations of an earlier write
/// of the colour runs many frames late, and Qt drew no frame for the number of a later write
/// of the source (1 run of 10).
#[test]
fn qt_kept_45_ms_in_every_step_stands_before_its_fades() {
    let dir = tempfile::tempdir().unwrap();
    write_icons(dir.path());
    let Some(run) = qt_run(dir.path(), Kept::EveryStep(45)) else {
        return;
    };
    let stood = stood_by_times(&run);
    eprintln!("Qt kept 45 ms in every step stood before its fades: {stood:?} frames; we stand 0");
    assert!(
        stood.iter().max() >= Some(&3),
        "Qt kept 45 ms in every step stood {stood:?} frames before its fades"
    );
}

/// `qt_fades` fails on a frame the real Qt did not draw in the frame its fade began in.
#[test]
fn a_frame_qt_did_not_draw_where_its_fade_began_fails_with_qt() {
    let dir = tempfile::tempdir().unwrap();
    write_icons(dir.path());
    let Some(run) = qt_run(dir.path(), Kept::WhereTheFadeBegins) else {
        return;
    };
    eprintln!("{}", undrawn_line(&run).unwrap_or_default());
    let failed = std::panic::catch_unwind(|| qt_fades(&run))
        .expect_err("Qt drew every frame its fades began in");
    let said = failed.downcast_ref::<String>().expect("a message");
    assert!(said.contains("the frame the fade began in"), "{said}");
}

#[test]
fn animated_icon_fades_into_its_new_source_like_qt() {
    let dir = tempfile::tempdir().unwrap();
    write_icons(dir.path());
    let Some(run) = qt_run(dir.path(), Kept::No) else {
        return;
    };
    let stood = qt_fades(&run);
    let qt = &run.frames;
    // F - (N + 3) of every fade: F is the first frame that shows it, N the frame of the write.
    eprintln!("Qt stood before its fades (frame of the write: frames): {stood:?}");
    if let Some(line) = undrawn_line(&run) {
        eprintln!("{line}");
    }

    // What Qt does with the first new source (frame 40): the plain icon shows it at once; the
    // animated one still shows the old image in that frame and the next (and in those Qt
    // stood for), then fades over 200 ms on InOutCubic.
    for k in 0..=17 {
        let Some(f) = qt.get(&(40 + k)) else {
            continue;
        };
        let m = match k.saturating_sub(stood[&40]) {
            0 | 1 => 0.0,
            k => in_out_cubic(((k - 1) as f64 * STEP / 200.0).min(1.0)),
        };
        let (old, new) = ((255.0 * (1.0 - m)).round() as u8, (255.0 * m).round() as u8);
        assert!(
            near(f[0], [old, 0, 0]) && near(f[1], [new, 0, 0]),
            "Qt, animated icon {k} frames after the write: {:?} {:?}, fade at {m}",
            f[0],
            f[1]
        );
        assert!(
            near(f[2], [0, old, 0]) && near(f[3], [0, new, 0]),
            "Qt, animated mask {k} frames after the write: {:?} {:?}, fade at {m}",
            f[2],
            f[3]
        );
        assert_eq!(
            [f[4], f[5]],
            [[0, 0, 0], [255, 0, 0]],
            "Qt, icon that is not animated, {k} frames after the write"
        );
        // Where the two images overlap, red goes over into blue.
        assert!(
            near(f[11], [old, 0, new]),
            "Qt, overlapping images {k} frames after the write: {:?}, fade at {m}",
            f[11]
        );
    }

    // Both of our renderers against Qt on OpenGL: the CPU one draws the same scene without a
    // GPU, it is not Qt's software backend (on which Kirigami does not fade).
    let names: Vec<String> = (SLOTS.iter())
        .flat_map(|(name, xs)| xs.iter().map(move |x| format!("{name} (x {x})")))
        .collect();
    let mut wrong = Vec::new();
    for (renderer, on_gpu) in [("cpu", false), ("gpu", true)] {
        let ours = our_frames(dir.path(), &run, &stood, on_gpu);
        for n in (FIRST..run.last).filter(|n| !run.undrawn.contains(n)) {
            for (k, name) in names.iter().enumerate() {
                let (o, q) = (ours[&n][k], qt[&n][k]);
                if !near(o, q) {
                    wrong.push(format!(
                        "{renderer}, frame {n}, {name}: ours {o:?}, Qt {q:?}"
                    ));
                }
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "{} pixels differ from Qt:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}
