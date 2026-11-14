//! The Material ProgressBar that `qml/` leaves to the style (StyledIndeterminateProgressBar,
//! and WIndeterminateProgressBar with `background: null`), against Qt 6.12.
//!
//! The bars that stand still are drawn by Qt itself next to ours (support/qt_window.rs). A
//! running bar is a function of the time since it was shown, which a live Qt cannot be asked
//! for twice, so its frames are kept in `progress_bar_qt/*.txt` with the times Qt drew them
//! at (tools/oracle/progress_oracle.py; the command is in each file): `running`, the two bars alone,
//! `reshown`, the same two hidden and shown again, and `places`, the places of `qml/` that
//! show a ProgressBar, running or with a value.
//!
//! Every comparison is of whole bytes: no pixel may differ.

#[path = "support/qt_window.rs"]
mod qt_window;

use qml::runtime::Runtime;
use qml::value::Value;
use qsg::raster::Canvas;
use shell::scene::{self, Target};
use std::path::{Path, PathBuf};

const IMPORTS: &str = "import QtQuick.Controls.Material\nimport QtQuick.Controls\n";
const DPRS: [f64; 3] = [1.0, 1.25, 2.0];

fn like_qt(body: &str) {
    qt_window::like_qt(&format!("{IMPORTS}{body}"), (300, 64), &DPRS, 0, 0);
}

/// The bar as StyledIndeterminateProgressBar writes it, with a value so that it stands still:
/// in its own size, and where its box is higher than the 4 px of the bar, at a whole and at
/// half a pixel.
#[test]
fn bar_and_track_lie_where_qt_puts_them() {
    like_qt(
        "ProgressBar { x: 10; y: 6; value: 0.4; Material.accent: \"#80c0ff\" }\n\
         ProgressBar { x: 10; y: 20; width: 130; height: 20; value: 0.4; Material.accent: \"#80c0ff\" }\n\
         ProgressBar { x: 150; y: 20; width: 130; height: 21; value: 0.4; Material.accent: \"#80c0ff\" }\n\
         ProgressBar { x: 10; y: 46; width: 130; height: 9; value: 1; Material.accent: \"#80c0ff\" }\n\
         ProgressBar { x: 150; y: 46; width: 130; height: 5; value: 0; Material.accent: \"#80c0ff\" }",
    );
}

/// `background: null` (WIndeterminateProgressBar): no track, only the bar.
#[test]
fn no_background_no_track() {
    like_qt(
        "ProgressBar { x: 10; y: 6; width: 240; implicitHeight: 3; value: 0.4; background: null; Material.accent: \"#80c0ff\" }\n\
         ProgressBar { x: 10; y: 20; width: 240; height: 9; value: 0.4; background: null; Material.accent: \"#80c0ff\" }\n\
         ProgressBar { x: 10; y: 40; width: 240; height: 4; value: 1; background: null; Material.accent: \"#80c0ff\" }",
    );
}

/// WIndeterminateProgressBar as it is written: 3 px high, no background, behind a mask with
/// round ends.
#[test]
fn masked_bar_without_background() {
    like_qt(
        "ProgressBar { id: bar; x: 10; y: 20; width: 240; implicitHeight: 3; value: 0.4; background: null\n\
             Material.accent: \"#80c0ff\"; layer.enabled: true\n\
             layer.effect: OpacityMask { maskSource: Rectangle { width: bar.width; height: bar.height; radius: bar.height / 2 } } }",
    );
}

/// The style's own parts move with the control's padding (the bar) and insets (the track).
#[test]
fn padding_moves_the_bar_and_insets_the_track() {
    like_qt(
        "ProgressBar { x: 10; y: 10; width: 240; height: 30; value: 0.4; Material.accent: \"#80c0ff\"\n\
             leftPadding: 20; rightPadding: 40; topPadding: 12; leftInset: 6; rightInset: 10; topInset: 4 }",
    );
}

struct Frame {
    /// Milliseconds from the first frame to each later render, the last the one kept.
    renders: Vec<f64>,
    /// Drawn while the bars were hidden (`hidden MS`).
    hidden: bool,
    px: Vec<[u8; 3]>,
}

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/progress_bar_qt")
}

/// (width, height, Qt's frames) of `scene`.txt.
fn qt_frames(scene: &str) -> (usize, usize, Vec<Frame>) {
    let text = std::fs::read_to_string(dir().join(format!("{scene}.txt"))).unwrap();
    let hex = |s: &str| {
        let v = u32::from_str_radix(s, 16).unwrap();
        [(v >> 16) as u8, (v >> 8) as u8, v as u8]
    };
    let span = |s: &str| {
        let (a, b) = s.split_once('-').unwrap();
        a.parse::<usize>().unwrap()..=b.parse::<usize>().unwrap()
    };
    let (mut w, mut h, mut frames) = (0, 0, Vec::<Frame>::new());
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        if let Some(size) = line.strip_prefix("size ") {
            let (a, b) = size.split_once(' ').unwrap();
            (w, h) = (a.parse().unwrap(), b.parse().unwrap());
        } else if line.starts_with("now ") {
            // qt_now reads it.
        } else if let Some((kind, ms)) = line.split_once(' ').filter(|l| !l.0.contains(':')) {
            assert!(["frame", "hidden"].contains(&kind), "{line}");
            frames.push(Frame {
                renders: ms.split(' ').map(|m| m.parse().unwrap()).collect(),
                hidden: kind == "hidden",
                px: vec![[0x20; 3]; w * h],
            });
        } else if !line.is_empty() {
            let (rows, runs) = line.split_once(": ").unwrap();
            let f = frames.last_mut().unwrap();
            for y in span(rows) {
                for run in runs.split(' ') {
                    let (xs, c) = run.split_once(':').unwrap();
                    for x in span(xs) {
                        f.px[y * w + x] = hex(c);
                    }
                }
            }
        }
    }
    assert!(frames.len() >= 6, "{} frames", frames.len());
    (w, h, frames)
}

/// What `Date.now()` answered in Qt's frames of `scene`.txt (`now MS`), if it was held.
fn qt_now(scene: &str) -> Option<f64> {
    let text = std::fs::read_to_string(dir().join(format!("{scene}.txt"))).unwrap();
    let now = text.lines().find_map(|l| l.strip_prefix("now "))?;
    Some(now.parse().unwrap())
}

/// A scene in a runtime of its own thread's, and what paints it.
struct Shown {
    rt: Runtime,
    root: usize,
    fonts: scene::Fonts,
    size: (usize, usize),
    gpu: Option<shell::gpu::Gpu>,
}

/// The clock reads this when the scene is shown.
const SHOWN_AT: f64 = 5000.0;

impl Shown {
    fn new(scene: &str, gpu: bool) -> Self {
        let mut rt = Runtime::new(Some(dir()));
        let root = rt
            .load_file(&dir().join(format!("{scene}.qml")))
            .expect("loads");
        let size = [rt.get(root, "width"), rt.get(root, "height")].map(|v| v.to_number() as usize);
        let gpu = gpu.then(|| {
            shell::gpu::Gpu::headless("surfaceless")
                .expect("surfaceless EGL/GLES2 context (Mesa)")
                .0
        });
        let mut s = Shown {
            rt,
            root,
            fonts: scene::Fonts::default(),
            size: (size[0], size[1]),
            gpu,
        };
        for _ in 0..3 {
            s.rt.settle();
            s.layout();
        }
        // updatePolish: a Canvas paints after the layouts placed it.
        if s.rt.canvases_available() {
            s.rt.settle();
            s.layout();
        }
        let errors = s.rt.errors();
        assert!(errors.is_empty(), "{errors:?}");
        s
    }

    fn layout(&mut self) {
        let size = [self.size.0 as f64, self.size.1 as f64];
        scene::layout_window(&mut self.rt, &mut self.fonts, self.root, size);
    }

    /// The window painted `ms` after it was shown, a running bar in it.
    fn paint(&mut self, ms: f64) -> Vec<[u8; 3]> {
        let (px, animating) = self.draw(ms);
        assert!(animating, "a running bar asks for the next frame");
        px
    }

    /// (the window painted `ms` after it was shown, whether it asks for the next frame).
    fn draw(&mut self, ms: f64) -> (Vec<[u8; 3]>, bool) {
        scene::set_wall_clock(Some(SHOWN_AT + ms));
        let (w, h) = self.size;
        let c = match &mut self.gpu {
            None => {
                let mut c = Canvas::new(w, h, [0, 0, 0, 0]);
                scene::paint_window(
                    &mut self.rt,
                    &mut self.fonts,
                    self.root,
                    &mut Target::Cpu(&mut c),
                );
                c
            }
            Some(g) => {
                let mut fr = shell::gpu::Frame::new(w, h);
                scene::paint_window(
                    &mut self.rt,
                    &mut self.fonts,
                    self.root,
                    &mut Target::Gpu(&mut fr),
                );
                let mut s = g.pbuffer(w as i32, h as i32).expect("pbuffer");
                s.reswap();
                g.render(&mut s, &mut fr, || {});
                g.read_surface(&s)
            }
        };
        let px = c.px.iter().map(|p| [p[0], p[1], p[2]]).collect();
        (px, scene::take_animating())
    }

    /// Shows or hides both bars.
    fn set_visible(&mut self, on: bool) {
        let bars = self.rt.world.borrow().items[self.root].children.clone();
        assert_eq!(bars.len(), 2);
        for b in bars {
            self.rt.set(b, "visible", Value::Bool(on));
        }
        self.rt.settle();
        self.layout();
    }
}

/// Where our pixels are not Qt's.
fn differing(ours: &[[u8; 3]], qt: &[[u8; 3]]) -> Vec<usize> {
    (0..qt.len()).filter(|&k| ours[k] != qt[k]).collect()
}

/// What differs between our pixels and Qt's, if anything.
fn differs(ours: &[[u8; 3]], qt: &[[u8; 3]], w: usize) -> Option<String> {
    let bad = differing(ours, qt);
    let k = *bad.first()?;
    Some(format!(
        "{} pixels, the first at ({}, {}): ours {:?}, Qt {:?}",
        bad.len(),
        k % w,
        k / w,
        ours[k],
        qt[k]
    ))
}

/// What we painted, to look at: target/tmp/progress_bar_qt/NAME.ppm.
fn keep(name: &str, px: &[[u8; 3]], (w, h): (usize, usize)) {
    let shots = Path::new(env!("CARGO_TARGET_TMPDIR")).join("progress_bar_qt");
    std::fs::create_dir_all(&shots).unwrap();
    let mut ppm = format!("P6 {w} {h} 255\n").into_bytes();
    ppm.extend(px.iter().flatten());
    std::fs::write(shots.join(format!("{name}.ppm")), ppm).unwrap();
}

/// Our frames of `scene` at the times of Qt's, each from a scene shown anew: (the times, Qt's
/// pixels, ours).
type Pair = (Vec<f64>, Vec<[u8; 3]>, Vec<[u8; 3]>);
fn ours_and_qt(scene: &'static str, gpu: bool) -> (usize, Vec<Pair>) {
    let (w, h, frames) = qt_frames(scene);
    let mut pairs = Vec::new();
    for (k, f) in frames.into_iter().enumerate() {
        let renders = f.renders.clone();
        // A thread per runtime: the scene's per-item state is the thread's.
        let got = std::thread::spawn(move || {
            qml::builtins::set_clock(qt_now(scene));
            let mut s = Shown::new(scene, gpu);
            assert_eq!(s.size, (w, h));
            let mut px = s.paint(0.0);
            for &ms in &renders {
                px = s.paint(ms);
            }
            keep(
                &format!("{scene}-{}-{k}", if gpu { "gpu" } else { "cpu" }),
                &px,
                (w, h),
            );
            px
        });
        pairs.push((f.renders, f.px, got.join().unwrap()));
    }
    (w, pairs)
}

/// Each of Qt's frames of `scene` is what we paint at the same times after the bars were
/// shown.
fn running_like_qt(scene: &'static str, gpu: bool) {
    let (w, pairs) = ours_and_qt(scene, gpu);
    let bad: Vec<String> = pairs
        .iter()
        .filter_map(|(ms, qt, ours)| differs(ours, qt, w).map(|d| format!("{ms:?} ms: {d}")))
        .collect();
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn a_running_bar_counts_from_when_it_was_shown() {
    running_like_qt("running", false);
}

#[test]
fn a_running_bar_counts_from_when_it_was_shown_gpu() {
    running_like_qt("running", true);
}

/// The places of places.qml: the line of `qml/`, the window's rectangle (x, y, right, bottom)
/// that holds what the lines around the bar place there, and the pixels that differ from
/// Qt's in the 11 frames together: none. The two places of waffle behind an OpacityMask had
/// one each while the GPU got a rectangle's vertices as window pixels and worked gl_Position
/// back out of them; it gets gl_Position itself now, as Qt's shader gives it.
const PLACES: [(&str, [usize; 4], usize); 12] = [
    (
        "rusty/sidebarRight/bluetoothDevices/BluetoothDialog.qml:26",
        [0, 0, 380, 70],
        0,
    ),
    (
        "rusty/sidebarRight/wifiNetworks/WifiDialog.qml:20",
        [0, 70, 380, 136],
        0,
    ),
    (
        "rusty/wallpaperSelector/WallpaperSelectorContent.qml:266",
        [0, 136, 380, 170],
        0,
    ),
    (
        "waffle/actionCenter/bluetooth/BluetoothControl.qml:72",
        [0, 170, 380, 218],
        0,
    ),
    (
        "waffle/actionCenter/wifi/WifiControl.qml:62",
        [0, 218, 380, 272],
        0,
    ),
    (
        "rusty/mediaControls/PlayerControl.qml:263",
        [0, 272, 380, 310],
        0,
    ),
    (
        "rusty/onScreenDisplay/OsdValueIndicator.qml:98",
        [0, 310, 380, 344],
        0,
    ),
    (
        "rusty/wallpaperSelector/WallpaperSelectorContent.qml:278",
        [0, 344, 380, 380],
        0,
    ),
    ("rusty/bar/BatteryIndicator.qml:21", [0, 380, 100, 440], 0),
    (
        "rusty/verticalBar/BatteryIndicator.qml:20",
        [100, 380, 160, 440],
        0,
    ),
    (
        "waffle/onScreenDisplay/OSDValue.qml:50",
        [160, 380, 380, 440],
        0,
    ),
    (
        "common/widgets/LightDarkPreferenceButton.qml:70",
        [0, 440, 380, 530],
        0,
    ),
];

/// The twelve places of `qml/` that places.qml shows (five with a running bar, seven with a
/// value, the last of them a wave drawn by the clock, which is held at what Qt's was), with
/// the lines that place the bar there. On the GPU: four are behind a mask. Every place has
/// something drawn in Qt's frames, and the pixels of it that differ in the 11 frames are as
/// many as `PLACES` says: none.
#[test]
fn the_places_of_qml_that_show_a_bar() {
    let (w, pairs) = ours_and_qt("places", true);
    assert_eq!(pairs.len(), 11, "frames");
    let within = |k: usize, r: [usize; 4]| {
        (r[0]..r[2]).contains(&(k % w)) && (r[1]..r[3]).contains(&(k / w))
    };
    let mut bad = Vec::new();
    for (name, r, held) in PLACES {
        let drawn = (0..pairs[0].1.len())
            .filter(|&k| within(k, r) && pairs[0].1[k] != [0x20; 3])
            .count();
        assert!(drawn > 0, "{name}: nothing of it in Qt's frame");
        let (mut differ, mut most) = (0, 0);
        for (_, qt, ours) in &pairs {
            for k in differing(ours, qt).into_iter().filter(|&k| within(k, r)) {
                differ += 1;
                let level = (0..3).map(|c| ours[k][c].abs_diff(qt[k][c])).max();
                most = most.max(level.unwrap());
            }
        }
        println!("{name}: {differ} pixels differ in {} frames", pairs.len());
        if differ != held || most > 1 {
            bad.push(format!(
                "{name}: {differ} pixels, not {held}; by {most} at most"
            ));
        }
    }
    let outside: usize = pairs
        .iter()
        .map(|(_, qt, ours)| {
            let all = differing(ours, qt);
            all.iter()
                .filter(|&&k| !PLACES.iter().any(|p| within(k, p.1)))
                .count()
        })
        .sum();
    assert_eq!(outside, 0, "pixels that differ outside every place");
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// Qt's bar runs while `control.visible && control.indeterminate`: hidden, nothing of it is
/// drawn and no frame is asked for; shown again, it starts from the beginning, whatever the
/// time it ran before. Qt's frames of reshown.txt: one before the bars were hidden, one while
/// they were, and those after they were shown again, each with the time of the frame that
/// found them running again.
fn shown_again_like_qt(gpu: bool) {
    let (w, h, frames) = qt_frames("reshown");
    let at = frames
        .iter()
        .position(|f| f.hidden)
        .expect("a hidden frame");
    let (before, hidden) = (&frames[at - 1], &frames[at]);
    assert_eq!(before.renders.len(), 1);
    assert!(
        hidden.px.iter().all(|p| *p == [0x20; 3]),
        "Qt drew a hidden bar"
    );
    let after = &frames[at + 1..];
    assert!(after.len() >= 6, "{} frames", after.len());
    assert!(after.iter().all(|f| !f.hidden && f.renders.len() >= 2));
    assert!(
        after.iter().any(|f| f.renders.len() == 3),
        "none past a round"
    );
    let mut bad = Vec::new();
    for (k, f) in frames[at + 1..].iter().enumerate() {
        // A thread per runtime: the scene's per-item state is the thread's.
        bad.extend(std::thread::scope(|t| {
            let shown = t.spawn(move || {
                let mut bad = Vec::new();
                let mut s = Shown::new("running", gpu);
                assert_eq!(s.size, (w, h));
                s.paint(0.0);
                let px = s.paint(before.renders[0]);
                bad.extend(differs(&px, &before.px, w).map(|d| format!("before: {d}")));
                s.set_visible(false);
                let (px, animating) = s.draw(hidden.renders[0]);
                assert!(!animating, "no bar is shown");
                bad.extend(differs(&px, &hidden.px, w).map(|d| format!("hidden: {d}")));
                s.set_visible(true);
                let mut px = Vec::new();
                for &ms in &f.renders {
                    px = s.paint(ms);
                }
                let name = format!("reshown-{}-{k}", if gpu { "gpu" } else { "cpu" });
                keep(&name, &px, (w, h));
                bad.extend(differs(&px, &f.px, w).map(|d| format!("{:?} ms: {d}", f.renders)));
                bad
            });
            shown.join().unwrap()
        }));
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn a_bar_shown_again_starts_again() {
    shown_again_like_qt(false);
}

#[test]
fn a_bar_shown_again_starts_again_gpu() {
    shown_again_like_qt(true);
}
