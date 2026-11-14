//! A Kirigami.Icon whose `source` changes after it has been drawn shows the new icon, as under
//! Qt (Kirigami's Icon::setSource polishes the item). Kirigami is not shipped and its Icon
//! resolves to Item, which declares no `source`, so the write did not count as drawn: the
//! recorded draw of the icon was kept, and no frame was asked for when nothing else changed.
//! The Waffle taskbar's speaker icon is `speaker-none` until PipeWire names the default sink;
//! when the sink arrived after the first frame, the taskbar kept that icon until the pointer
//! came over it. `color` and `isMask` are set the same way (Icon::setColor, Icon::setIsMask).
//! Compared with Qt itself: Qt Quick with Kirigami through OpenGL (`support/qt_gl.rs`), pixel
//! by pixel.

#[path = "support/qt_gl.rs"]
mod qt_gl;

use qml::runtime::Runtime;
use qml::value::Value;
use qsg::raster::Canvas;
use shell::scene::{self, Target};
use std::path::Path;

const W: usize = 96;
/// The icons are 32 high; a window under sway is no lower than 50.
const H: usize = 64;

/// Two icons that fill one half of their square each: `a` the left one, `b` the right one.
fn write_icons(dir: &Path) {
    for (name, x) in [("a", 0), ("b", 16)] {
        let svg = format!(
            "<svg width=\"32\" height=\"32\" viewBox=\"0 0 32 32\" \
             xmlns=\"http://www.w3.org/2000/svg\">\
             <rect x=\"{x}\" y=\"0\" width=\"16\" height=\"32\" fill=\"#ff0000\"/></svg>\n"
        );
        std::fs::write(dir.join(format!("{name}.svg")), svg).unwrap();
    }
}

/// A plain icon and one set up as the original's FluentIcon (a mask, `animated`), in an item of their
/// own, and a square beside them. `driver` is what changes the scene under Qt.
fn source(dir: &Path, driver: &str) -> String {
    let dir = dir.display();
    format!(
        r##"import QtQuick
import org.kde.kirigami as Kirigami
Item {{
    id: root
    width: {W}; height: {H}
    property string shown: "a"
    property bool lit: false
    property color tint: "#00ff00"
    property bool mask: true
    Rectangle {{ anchors.fill: parent; color: "#101010" }}
    Item {{
        width: 64; height: 32
        Kirigami.Icon {{
            width: 32; height: 32; roundToIconSize: false
            source: "{dir}/" + root.shown + ".svg"
        }}
        Kirigami.Icon {{
            x: 32; width: 32; height: 32; roundToIconSize: false
            source: "{dir}/" + root.shown + ".svg"
            isMask: root.mask; color: root.tint; animated: true
        }}
    }}
    Rectangle {{ x: 64; width: 32; height: 32; color: root.lit ? "#ffffff" : "#000000" }}
{driver}}}
"##
    )
}

/// Root properties written in one step, as JavaScript literals.
type Step = &'static [(&'static str, &'static str)];

/// The scene as loaded, then the icons' source changed together with the square (the window is
/// drawn again for the square too), then the source changed alone.
const SOURCE_STEPS: [Step; 3] = [
    &[],
    &[("shown", "\"b\""), ("lit", "true")],
    &[("shown", "\"a\"")],
];

/// The scene as loaded, then the mask icon's colour changed, then its mask taken off.
const COLOR_STEPS: [Step; 3] = [&[], &[("tint", "\"#ffff00\"")], &[("mask", "false")]];

fn value(js: &str) -> Value {
    match js.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        Some(s) => Value::str(s),
        None => Value::Bool(js == "true"),
    }
}

/// The scene as Qt Quick shows it after each step, once the mask icon's cross-fade is over.
fn qt_frames(dir: &Path, steps: &[Step]) -> Vec<Canvas> {
    // Once the first frame is on screen: wait, shoot, write the next step's properties.
    let mut chain = String::from("root.finished");
    for step in steps[1..].iter().rev() {
        let set: String = step
            .iter()
            .map(|(k, v)| format!("root.{k} = {v}; "))
            .collect();
        chain = format!("() => {{ {set}root.step({chain}); }}");
    }
    let driver = format!(
        "    signal shoot()\n    signal finished()\n    property var after\n    \
         property bool drawn: false\n    \
         Timer {{ id: wait; interval: 700; onTriggered: root.after() }}\n    \
         function step(next) {{\n        \
         root.after = () => {{ root.shoot(); next(); }};\n        wait.restart();\n    }}\n    \
         Connections {{\n        target: root.Window.window\n        \
         function onFrameSwapped() {{\n            if (root.drawn)\n                return;\n            \
         root.drawn = true;\n            root.step({chain});\n        }}\n    }}\n"
    );
    let (log, shots) = qt_gl::steps(&source(dir, &driver));
    assert_eq!(shots.len(), steps.len(), "Qt printed:\n{log}");
    for c in &shots {
        assert_eq!((c.w, c.h), (W, H), "Qt printed:\n{log}");
    }
    shots
}

/// The same steps under our engine, drawn as the shell draws a window: a frame only when a
/// write touched something, with the recorded draws of what was touched dropped first. Each
/// step is followed by the frames of the 700 ms Qt's grab waits (the mask icon's fade).
fn our_frames(dir: &Path, steps: &[Step]) -> Vec<Canvas> {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(&source(dir, "")).expect("scene loads");
    let mut fonts = scene::Fonts::default();
    let (mut gpu, _) =
        shell::gpu::Gpu::headless("surfaceless").expect("surfaceless EGL/GLES2 context (Mesa)");
    let mut surface = gpu.pbuffer(W as i32, H as i32).expect("pbuffer");
    let mut frames: Vec<Canvas> = Vec::new();
    let mut last: Option<Canvas> = None;
    let mut now = 0.0;
    for step in steps {
        for (k, v) in *step {
            rt.world.borrow_mut().write(root, k, value(v));
        }
        for _ in 0..=700 / 16 {
            now += 16.0;
            rt.tick(now);
            let (touched, moved) = {
                let mut w = rt.world.borrow_mut();
                (std::mem::take(&mut w.touched), std::mem::take(&mut w.moved))
            };
            if last.is_some() && touched.is_empty() && moved.is_empty() {
                continue;
            }
            {
                let w = rt.world.borrow();
                scene::invalidate_moved(&w, &moved);
                scene::invalidate_layers(&w, &touched);
            }
            // The first frame twice: the second one replays what the first recorded.
            for _ in 0..if last.is_none() { 2 } else { 1 } {
                scene::layout_window(&mut rt, &mut fonts, root, [W as f64, H as f64]);
                rt.settle();
                let mut fr = shell::gpu::Frame::new(W, H);
                scene::paint_window(&mut rt, &mut fonts, root, &mut Target::Gpu(&mut fr));
                surface.reswap();
                gpu.render(&mut surface, &mut fr, || {});
            }
            last = Some(gpu.read_surface(&surface));
        }
        frames.push(last.clone().unwrap());
    }
    frames
}

fn rgb(c: &Canvas, x: usize, y: usize) -> [u8; 3] {
    let p = c.px[y * W + x];
    [p[0], p[1], p[2]]
}

/// The pixels of ours that are not Qt's, frame by frame.
fn unlike_qt(ours: &[Canvas], qt: &[Canvas]) -> Vec<String> {
    assert_eq!(ours.len(), qt.len());
    let mut wrong = Vec::new();
    for (n, (a, b)) in ours.iter().zip(qt).enumerate() {
        let differ: Vec<usize> = (0..W * H).filter(|&i| a.px[i] != b.px[i]).collect();
        if let Some(&i) = differ.first() {
            wrong.push(format!(
                "after step {n}: {} pixels differ, the first at ({}, {}): {:?}, Qt has {:?}",
                differ.len(),
                i % W,
                i / W,
                a.px[i],
                b.px[i]
            ));
        }
    }
    wrong
}

#[test]
fn icon_shows_its_new_source_like_qt() {
    if !qt_gl::available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    write_icons(dir.path());
    let qt = qt_frames(dir.path(), &SOURCE_STEPS);
    // The middle of each half of the plain icon, then of the mask, which is tinted green.
    let halves: Vec<[[u8; 3]; 4]> = qt
        .iter()
        .map(|c| [8, 24, 40, 56].map(|x| rgb(c, x, 16)))
        .collect();
    let (red, green, none) = ([255, 0, 0], [0, 255, 0], [0x10; 3]);
    assert_eq!(
        halves,
        [
            [red, none, green, none],
            [none, red, none, green],
            [red, none, green, none]
        ],
        "Qt draws the icon of the source it was last given"
    );
    let wrong = unlike_qt(&our_frames(dir.path(), &SOURCE_STEPS), &qt);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// A mask icon is filled with its `color`, and is the file's own colours once `isMask` is off:
/// both writes are drawn. Kirigami's setters polish the icon like setSource.
#[test]
fn icon_shows_its_new_color_and_mask() {
    if !qt_gl::available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    write_icons(dir.path());
    let qt = qt_frames(dir.path(), &COLOR_STEPS);
    let mask: Vec<[u8; 3]> = qt.iter().map(|c| rgb(c, 40, 16)).collect();
    assert_eq!(
        mask,
        [[0, 255, 0], [255, 255, 0], [255, 0, 0]],
        "Qt draws the mask in the colour it was last given, then the file's own"
    );
    let wrong = unlike_qt(&our_frames(dir.path(), &COLOR_STEPS), &qt);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
