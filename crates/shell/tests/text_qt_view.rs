//! `Text.QtRendering` whose glyphs lie outside the window, or outside what the window shows of
//! them, pixel by pixel against Qt Quick through OpenGL (support/qt_gl.rs), ours on the CPU and
//! on the GPU: a glyph is left out by the rectangle of what it is drawn into (a layer's
//! texture, the window), not by the window's alone. Text in a layer larger than the window and
//! hanging over its edges, text of a source that stands beyond the window's edge and is shown
//! inside it by OpacityMask and GaussianBlur, and text in a Flickable scrolled from beyond its
//! edge: as the scene loads, and between two frames.
//!
//! Text that is moved between two frames is also where Qt's batch renderer changes how it
//! places the glyphs (the moved node becomes a batch root once enough lies under it), so the
//! scenes of `MOVED` stand on both sides of its two limits.
//!
//! `tools/testenv/run.sh cargo nextest run -p shell --features interp --test text_qt_view`

#[path = "support/qt_gl.rs"]
mod qt_gl;

use qml::value::Value;
use qsg::raster::Canvas;
use shell::scene::{self, Target};

const SIZE: (usize, usize) = (320, 240);

const FONT: &str = "font.family: \"Noto Sans\"; renderType: Text.QtRendering";

/// Wider than the window by far at 16 px: the glyphs differ along it, so a text drawn from
/// another place of it shows.
fn long() -> String {
    "The quick brown fox jumps over the lazy dog 0123456789 WijmAB. ".repeat(8)
}

/// A scene `SIZE` large whose root has `across` and `down`, which its tests move: the QML
/// with those at `at` and with `driver` in the root, and the parts of the window to look at.
struct Scene {
    source: fn(at: [f64; 2], driver: &str) -> String,
    parts: &'static [(&'static str, [usize; 4])],
}

fn root(at: [f64; 2], body: &str, driver: &str) -> String {
    format!(
        r##"import QtQuick
import Qt5Compat.GraphicalEffects
Rectangle {{
    id: root
    width: {w}; height: {h}; color: "white"
    property real across: {ax}
    property real down: {ay}
{body}{driver}}}
"##,
        w = SIZE.0,
        h = SIZE.1,
        ax = at[0],
        ay = at[1],
    )
}

/// Layers and sources larger than the window or beyond its edge, and two Flickables.
const VIEW: Scene = Scene {
    source: |at, driver| {
        let long = long();
        let lines: Vec<String> = (0..120).map(|n| format!("line {n} ABWijm")).collect();
        let lines = lines.join("\\n");
        let body = format!(
            r##"    // A layer larger than the window, over both of its sides.
    Rectangle {{ id: wide; x: 700; y: 580; width: 500; height: 26; radius: 13; visible: false }}
    Item {{
        x: -90; y: 2; width: 500; height: 26
        layer.enabled: true
        layer.effect: OpacityMask {{ maskSource: wide }}
        Text {{ x: 3; y: 2; text: "{long}"; font.pixelSize: 16; {FONT} }}
    }}
    // A layer over the right and the lower edge, its text begins inside.
    Rectangle {{ id: tall; x: 700; y: 620; width: 200; height: 60; radius: 20; visible: false }}
    Item {{
        x: 236; y: 206; width: 200; height: 60
        layer.enabled: true
        layer.effect: OpacityMask {{ maskSource: tall }}
        Text {{ x: 3.5; y: 4.25; text: "ABWijm ABWijm ABWijm"; font.pixelSize: 20; {FONT} }}
    }}
    // Sources beyond the window's edge, shown inside it.
    Item {{
        id: off; x: 700; y: 500; width: 150; height: 26
        Text {{ x: 2.5; y: 3.25; text: "ABWijm fox 0123"; font.pixelSize: 16; {FONT} }}
    }}
    Item {{
        id: hidden; x: -900; y: -300; width: 150; height: 26; visible: false
        Text {{ x: 2.5; y: 3.25; text: "ABWijm fox 0123"; font.pixelSize: 16; color: "#c00000"; {FONT} }}
    }}
    Rectangle {{ id: round; x: 700; y: 540; width: 150; height: 26; radius: 13; visible: false }}
    OpacityMask {{ x: 162; y: 32; width: 150; height: 26; source: off; maskSource: round }}
    GaussianBlur {{ x: 6; y: 62; width: 150; height: 26; source: off; radius: 2; samples: 5 }}
    GaussianBlur {{ x: 162; y: 62; width: 150; height: 26; source: hidden; radius: 2; samples: 5 }}
    // Scrolled across and down from beyond the edge.
    Flickable {{
        x: 6; y: 94; width: 300; height: 26; clip: true
        contentWidth: 4000; contentHeight: 26; contentX: root.across
        Text {{ x: 3; y: 2; text: "{long}"; font.pixelSize: 16; {FONT} }}
    }}
    Flickable {{
        x: 6; y: 124; width: 220; height: 78; clip: true
        contentWidth: 220; contentHeight: 3000; contentY: root.down
        Text {{ x: 3.5; y: 2; text: "{lines}"; font.pixelSize: 16; {FONT} }}
    }}
"##
        );
        root(at, &body, driver)
    },
    parts: &[
        ("layer over both sides", [0, 2, 320, 26]),
        ("layer over the corner", [236, 206, 84, 34]),
        ("OpacityMask", [162, 32, 150, 26]),
        ("GaussianBlur", [6, 62, 150, 26]),
        ("GaussianBlur of a hidden source", [162, 62, 150, 26]),
        ("Flickable across", [6, 94, 300, 26]),
        ("Flickable down", [6, 124, 220, 78]),
    ],
};

/// Where the Flickables of `VIEW` stand, frame after frame: both beyond what the frame before
/// drew.
const STEPS: [[f64; 2]; 3] = [[0.0, 0.0], [1500.0, 900.5], [310.25, 2100.0]];

/// `count` glyphs that have an outline, a space after every fifth: Qt's glyph node has four
/// vertices for each of the `count` and none for a space.
fn glyphs(count: usize) -> String {
    let mut text = String::new();
    for n in 0..count {
        text.push(b"ABWjm0123456789"[n % 15] as char);
        if n % 5 == 4 && n + 1 < count {
            text.push(' ');
        }
    }
    text
}

/// Text that `across` moves, on both sides of the batch renderer's two limits (more than
/// 1024 vertices, more than 64 geometry nodes under the node that moved): 256 and 257 glyphs
/// in a Flickable, a text and 63 and 64 Rectangles in a Flickable, and, with no clip above
/// them, Items of three short texts (one in the window at each step, none across its edge)
/// and, below the window, 257 glyphs more, 62 Rectangles, 61 Rectangles; then a Text of 257
/// glyphs that moves itself.
const MOVED: Scene = Scene {
    source: |at, driver| {
        let flick = |y: usize, text: &str| {
            format!(
                r##"    Flickable {{
        x: 6; y: {y}; width: 300; height: 24; clip: true
        contentWidth: 5000; contentHeight: 24; contentX: root.across
        Text {{ x: 3.5; y: 1; text: "{text}"; font.pixelSize: 16; {FONT} }}
    }}
"##
            )
        };
        let dots = |y: usize, count: usize| {
            format!(
                r##"    Flickable {{
        x: 6; y: {y}; width: 300; height: 28; clip: true
        contentWidth: 5000; contentHeight: 28; contentX: root.across
        Text {{ x: 3.5; y: 0; text: "{text}"; font.pixelSize: 16; {FONT} }}
        Repeater {{
            model: {count}
            Rectangle {{ x: 4 + index * 37; y: 24; width: 21; height: 3; color: "#c00000" }}
        }}
    }}
"##,
                text = glyphs(180)
            )
        };
        let free = |y: usize, more: &str| {
            format!(
                r##"    Item {{
        x: 6.25 - root.across; y: {y}; width: 4000; height: 22
        Text {{ x: 3.5; y: 0; text: "{few}"; font.pixelSize: 16; {FONT} }}
        Text {{ x: 1403.5; y: 0; text: "{few}"; font.pixelSize: 16; {FONT} }}
        Text {{ x: 350; y: 0; text: "{few}"; font.pixelSize: 16; {FONT} }}
{more}    }}
"##,
                few = glyphs(22)
            )
        };
        let below = |count: usize| {
            format!(
                "        Repeater {{\n            model: {count}\n            \
                 Rectangle {{ x: index * 37; y: 400; width: 21; height: 3; color: \"#c00000\" }}\n        \
                 }}\n"
            )
        };
        let many = format!(
            "        Text {{ x: 3.5; y: 400; text: \"{}\"; font.pixelSize: 16; {FONT} }}\n",
            glyphs(257)
        );
        let body = [
            flick(2, &glyphs(256)),
            flick(28, &glyphs(257)),
            dots(54, 63),
            dots(84, 64),
            free(114, &many),
            free(136, &below(62)),
            free(158, &below(61)),
            format!(
                r##"    Text {{
        x: 9.75 + root.across / 256; y: 182; width: 280; wrapMode: Text.WrapAnywhere
        text: "{}"; font.pixelSize: 7; {FONT}
    }}
"##,
                glyphs(257)
            ),
        ]
        .concat();
        root(at, &body, driver)
    },
    parts: &[
        ("Flickable of 256 glyphs", [6, 2, 300, 24]),
        ("Flickable of 257 glyphs", [6, 28, 300, 24]),
        ("Flickable of text and 63 Rectangles", [6, 54, 300, 28]),
        ("Flickable of text and 64 Rectangles", [6, 84, 300, 28]),
        ("Item of texts and 257 glyphs", [0, 114, 320, 22]),
        ("Item of texts and 62 Rectangles", [0, 136, 320, 22]),
        ("Item of texts and 61 Rectangles", [0, 158, 320, 22]),
        ("Text of 257 glyphs", [0, 182, 320, 58]),
    ],
};

/// Where `across` puts the texts of `MOVED`: every one shows glyphs at each.
const MOVES: [[f64; 2]; 3] = [[0.0, 0.0], [1400.25, 0.0], [310.5, 0.0]];

/// Text in a layer that has no effect: Qt draws the item into a transparent texture and that
/// into the window, so the glyphs have nothing opaque under them there and come out gray.
const LAYERS: Scene = Scene {
    source: |at, driver| {
        let long = long();
        let body = format!(
            r##"    Item {{
        x: -90; y: 2; width: 500; height: 26; layer.enabled: true
        Text {{ x: 3; y: 2; text: "{long}"; font.pixelSize: 16; {FONT} }}
    }}
    Item {{
        x: 236; y: 206; width: 200; height: 60; layer.enabled: true
        Text {{ x: 3.5; y: 4.25; text: "ABWijm ABWijm ABWijm"; font.pixelSize: 20; {FONT} }}
    }}
"##
        );
        root(at, &body, driver)
    },
    parts: &[
        ("layer over both sides", [0, 2, 320, 26]),
        ("layer over the corner", [236, 206, 84, 34]),
    ],
};

/// What rusty-impulse does not draw as Qt does (docs/qt-differences.md): a ShaderEffectSource that
/// is on screen itself.
const SOURCE_ON_SCREEN: Scene = Scene {
    source: |at, driver| {
        let body = format!(
            r##"    Item {{
        id: off; x: 700; y: 500; width: 150; height: 26
        Text {{ x: 2.5; y: 3.25; text: "ABWijm fox 0123"; font.pixelSize: 16; {FONT} }}
    }}
    ShaderEffectSource {{ x: 6; y: 32; width: 150; height: 26; sourceItem: off }}
"##
        );
        root(at, &body, driver)
    },
    parts: &[("ShaderEffectSource", [6, 32, 150, 26])],
};

/// The pixels of the part of `SOURCE_ON_SCREEN` that are not Qt's, and by how much of 255 at
/// most.
const SOURCE_ON_SCREEN_DIFFERS: &[(usize, u8)] = &[(743, 255)];

/// What rusty-impulse does not draw as Qt does either (docs/qt-differences.md): the glyphs that an
/// edge of the window cuts, at each of its four edges.
const EDGES: Scene = Scene {
    source: |at, driver| {
        let text = glyphs(40);
        let body = format!(
            r##"    Text {{ x: 180.5; y: 30; text: "{text}"; font.pixelSize: 16; {FONT} }}
    Text {{ x: -150.25; y: 70; text: "{text}"; font.pixelSize: 16; {FONT} }}
    Text {{ x: 20.5; y: -9.5; width: 280; text: "{text}"; font.pixelSize: 16; elide: Text.ElideRight; {FONT} }}
    Text {{ x: 20.5; y: 227.25; width: 280; text: "{text}"; font.pixelSize: 16; elide: Text.ElideRight; {FONT} }}
"##
        );
        root(at, &body, driver)
    },
    parts: &[
        ("text over the right edge", [160, 30, 160, 24]),
        ("text over the left edge", [0, 70, 160, 24]),
        ("text over the upper edge", [0, 0, 320, 16]),
        ("text over the lower edge", [0, 224, 320, 16]),
    ],
};

/// The pixels of each part of `EDGES` that are not Qt's, and by how much of 255 at most.
const EDGES_DIFFER: &[(usize, u8)] = &[(0, 0), (0, 0), (23, 4), (41, 2)];

/// Drops the recorded draws that the runtime's changes since the last picture made stale, as
/// the shell does before each frame.
fn invalidate(rt: &mut qml::runtime::Runtime) {
    let (restructured, moved, touched) = {
        let mut w = rt.world.borrow_mut();
        let w = &mut *w;
        (
            std::mem::take(&mut w.restructured),
            std::mem::take(&mut w.moved),
            std::mem::take(&mut w.touched),
        )
    };
    let w = rt.world.borrow();
    scene::invalidate_parents(&w, &restructured);
    scene::invalidate_moved(&w, &moved);
    scene::invalidate_layers(&w, &touched);
}

/// The scene loaded at `steps[0]` under our engine and moved to each next step, drawn as the
/// shell draws a window: on the CPU, or on the GPU (llvmpipe, as Qt's).
fn ours(scene: &Scene, steps: &[[f64; 2]], gpu: bool) -> Vec<Canvas> {
    let mut rt = qml::runtime::Runtime::new(None);
    let mut fonts = scene::Fonts::default();
    let root = rt
        .load_str(&(scene.source)(steps[0], ""))
        .expect("scene loads");
    let mut g = gpu.then(|| {
        let (g, _) =
            shell::gpu::Gpu::headless("surfaceless").expect("surfaceless EGL/GLES2 context (Mesa)");
        let s = g.pbuffer(SIZE.0 as i32, SIZE.1 as i32).expect("pbuffer");
        (g, s)
    });
    let mut frames = Vec::new();
    let mut now = 0.0;
    for (n, at) in steps.iter().enumerate() {
        if n > 0 {
            let mut w = rt.world.borrow_mut();
            w.write(root, "across", Value::Num(at[0]));
            w.write(root, "down", Value::Num(at[1]));
        }
        let mut last = None;
        // Twice: the second frame replays what the first recorded.
        for _ in 0..2 {
            now += 16.0;
            rt.tick(now);
            for _ in 0..2 {
                rt.settle();
                scene::layout_window(&mut rt, &mut fonts, root, [SIZE.0 as f64, SIZE.1 as f64]);
            }
            invalidate(&mut rt);
            last = Some(match &mut g {
                None => {
                    let mut picture = Canvas::new(SIZE.0, SIZE.1, [0, 0, 0, 0]);
                    scene::paint_window(&mut rt, &mut fonts, root, &mut Target::Cpu(&mut picture));
                    picture
                }
                Some((g, s)) => {
                    let mut fr = shell::gpu::Frame::new(SIZE.0, SIZE.1);
                    scene::paint_window(&mut rt, &mut fonts, root, &mut Target::Gpu(&mut fr));
                    s.reswap();
                    g.render(s, &mut fr, || {});
                    g.read_surface(s)
                }
            });
        }
        frames.extend(last);
    }
    frames
}

/// The scene in Qt Quick at each of `steps`: one picture of it loaded there, or, for several,
/// one after each move of a window that stays open.
fn qt(scene: &Scene, steps: &[[f64; 2]]) -> Vec<Canvas> {
    if let [at] = steps {
        return vec![qt_gl::run(&(scene.source)(*at, "")).1];
    }
    // Once the first frame is on screen: wait, shoot, move to the next step.
    let mut chain = String::from("root.finished");
    for at in steps[1..].iter().rev() {
        chain = format!(
            "() => {{ root.across = {}; root.down = {}; root.step({chain}); }}",
            at[0], at[1]
        );
    }
    let driver = format!(
        "    signal shoot()\n    signal finished()\n    property var after\n    \
         property bool drawn: false\n    \
         Timer {{ id: wait; interval: 300; onTriggered: root.after() }}\n    \
         function step(next) {{\n        \
         root.after = () => {{ root.shoot(); next(); }};\n        wait.restart();\n    }}\n    \
         Connections {{\n        target: root.Window.window\n        \
         function onFrameSwapped() {{\n            if (root.drawn)\n                return;\n            \
         root.drawn = true;\n            root.step({chain});\n        }}\n    }}\n"
    );
    let (log, shots) = qt_gl::steps(&(scene.source)(steps[0], &driver));
    assert_eq!(shots.len(), steps.len(), "Qt printed:\n{log}");
    shots
}

/// How ours differs from Qt in one part of the window at one step.
struct Part {
    step: usize,
    name: &'static str,
    /// Pixels that are not Qt's, and the most one differs by, of 255.
    count: usize,
    most: u8,
}

/// Ours against Qt at every step: each part of the scene, in which Qt shows text (pixels that
/// are not the white under it), and after the parts of a step the whole window (`name` "").
fn differences(scene: &Scene, steps: &[[f64; 2]], gpu: bool) -> Option<Vec<Part>> {
    if !(testenv::require("qml6") && qt_gl::available()) {
        return None;
    }
    let (qt, ours) = (qt(scene, steps), ours(scene, steps, gpu));
    let on = if gpu { "gpu" } else { "cpu" };
    let mut found = Vec::new();
    for (step, (q, o)) in qt.iter().zip(&ours).enumerate() {
        assert_eq!((q.w, q.h), SIZE);
        assert_eq!((o.w, o.h), SIZE);
        if let Ok(dir) = std::env::var("RI_TEXT_QT_VIEW_DUMP") {
            std::fs::create_dir_all(&dir).unwrap();
            for (name, p) in [("qt", q), ("ours", o)] {
                let raw: Vec<u8> = p.px.iter().flatten().copied().collect();
                let file = format!("{on}.{step}of{}.{}.{name}", steps.len(), steps[step][0]);
                std::fs::write(format!("{dir}/{file}.{}x{}.rgba", p.w, p.h), raw).unwrap();
            }
        }
        for &(name, [x, y, w, h]) in scene.parts {
            let at = |c: &Canvas, x: usize, y: usize| c.px[y * SIZE.0 + x];
            let cells = (y..y + h).flat_map(|y| (x..x + w).map(move |x| (x, y)));
            let inked = cells
                .clone()
                .filter(|&(x, y)| at(q, x, y) != [255; 4])
                .count();
            assert!(
                inked > 100,
                "step {step}: Qt draws the {name}: {inked} pixels"
            );
            let (mut count, mut most) = (0, 0);
            for (x, y) in cells {
                let (a, b) = (at(q, x, y), at(o, x, y));
                let d = (0..4).map(|k| a[k].abs_diff(b[k])).max().unwrap_or(0);
                count += (d > 0) as usize;
                most = most.max(d);
            }
            found.push(Part {
                step,
                name,
                count,
                most,
            });
        }
        let count = (0..SIZE.0 * SIZE.1).filter(|&i| q.px[i] != o.px[i]).count();
        found.push(Part {
            step,
            name: "",
            count,
            most: 0,
        });
    }
    Some(found)
}

/// Ours has Qt's pixels at every step.
fn compare(scene: &Scene, steps: &[[f64; 2]], gpu: bool) {
    let Some(found) = differences(scene, steps, gpu) else {
        return;
    };
    let wrong: Vec<String> = found
        .iter()
        .filter(|p| p.count > 0)
        .map(|p| match p.name {
            "" => format!(
                "step {}, the whole window: {} pixels differ",
                p.step, p.count
            ),
            name => format!(
                "step {}, {name}: {} pixels differ, by up to {} of 255",
                p.step, p.count, p.most
            ),
        })
        .collect();
    let on = if gpu { "gpu" } else { "cpu" };
    assert!(wrong.is_empty(), "on {on}:\n{}", wrong.join("\n"));
}

/// Ours differs from Qt as recorded: in each part of the scene by `recorded` (pixels, the
/// most of 255).
fn differs(scene: &Scene, recorded: &[(usize, u8)], gpu: bool) {
    let Some(found) = differences(scene, &STEPS[..1], gpu) else {
        return;
    };
    let found: Vec<(usize, u8)> = found
        .iter()
        .filter(|p| !p.name.is_empty())
        .map(|p| (p.count, p.most))
        .collect();
    assert_eq!(found, recorded);
}

macro_rules! targets {
    ($($name:ident: $gpu:expr;)*) => {
        $(mod $name {
            use super::*;

            #[test]
            fn text_outside_the_window_matches_qt_as_the_scene_loads() {
                compare(&VIEW, &STEPS[..1], $gpu);
                compare(&VIEW, &STEPS[1..2], $gpu);
            }

            #[test]
            fn text_scrolled_in_between_frames_matches_qt() {
                compare(&VIEW, &STEPS, $gpu);
            }

            #[test]
            fn text_moved_matches_qt_as_the_scene_loads() {
                compare(&MOVED, &MOVES[1..2], $gpu);
            }

            #[test]
            fn text_moved_between_frames_matches_qt_on_both_sides_of_a_batch_root() {
                compare(&MOVED, &MOVES, $gpu);
            }

            #[test]
            fn text_in_a_layer_without_an_effect_matches_qt() {
                compare(&LAYERS, &STEPS[..1], $gpu);
            }

            /// The difference docs/qt-differences.md records, to the pixel: when a
            /// ShaderEffectSource on screen is drawn as Qt draws it, this fails and the test
            /// below it takes its place.
            #[test]
            fn a_shader_effect_source_on_screen_differs_from_qt_as_recorded() {
                differs(&SOURCE_ON_SCREEN, SOURCE_ON_SCREEN_DIFFERS, $gpu);
            }

            #[test]
            #[ignore = "known difference (docs/qt-differences.md): a ShaderEffectSource on \
                        screen is not drawn as Qt draws it"]
            fn a_shader_effect_source_on_screen_matches_qt() {
                compare(&SOURCE_ON_SCREEN, &STEPS[..1], $gpu);
            }

            /// As above, for the glyphs an edge of the window cuts.
            #[test]
            fn text_cut_by_an_edge_of_the_window_differs_from_qt_as_recorded() {
                differs(&EDGES, EDGES_DIFFER, $gpu);
            }

            #[test]
            #[ignore = "known difference (docs/qt-differences.md): a glyph that an edge of \
                        the window cuts is not drawn from the cut triangles, as llvmpipe does"]
            fn text_cut_by_an_edge_of_the_window_matches_qt() {
                compare(&EDGES, &STEPS[..1], $gpu);
            }
        })*
    };
}

targets! {
    cpu: false;
    gpu: true;
}
