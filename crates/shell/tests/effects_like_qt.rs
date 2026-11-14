//! Qt5Compat DropShadow and GaussianBlur, RectangularShadow and MultiEffect paint what Qt 6.12
//! paints when their QML leaves properties unset, and MultiEffect's shadow is Qt's. The other
//! pixel tests of these effects compare our GPU path with our CPU path, and both take the
//! defaults from the same place; here the same items are drawn by Qt itself (`qml6` on OpenGL
//! under the test compositor, read back with `grabToImage`) and by each of our paths.
//!
//! RectangularShadow's `antialiasing` is not a plain default (qquickrectangularshadow.cpp):
//! `initialize()` turns it on, a later change of `radius` sets it to `radius > 0`, and the
//! picture keeps what `updateSizeProperties()` last read, so `antialiasing` written alone
//! shows only after the next change of size, `radius`, `blur`, `spread` or `offset`.

use qsg::raster::Canvas;
use shell::scene::{self, Target};

const BG: &str = "#203040";

/// Our 8-bit rounding of a blended colour is a level or two off Qt's; a property read wrongly
/// moves an edge by a pixel, which is tens of levels.
const TOL: u8 = 2;

/// One effect per cell, `cols` cells a row.
struct Scene {
    cell: (usize, usize),
    cols: usize,
    /// Inline components of the window.
    types: &'static str,
    /// Declarations before the cells (the effects' source).
    head: &'static str,
    /// Name and QML of each cell's content.
    cells: &'static [(&'static str, &'static str)],
    /// Body of `function step()`, run between the two pictures.
    step: &'static str,
}

impl Scene {
    fn size(&self) -> (usize, usize) {
        let rows = self.cells.len().div_ceil(self.cols);
        (self.cols * self.cell.0, rows * self.cell.1)
    }

    /// The `Rectangle` both engines draw.
    fn body(&self) -> String {
        let (w, h) = self.size();
        let mut s = format!(
            "    Rectangle {{\n        id: scene\n        width: {w}; height: {h}\n        \
             color: \"{BG}\"\n        function step() {{ {} }}\n{}",
            self.step, self.head
        );
        for (k, (_, qml)) in self.cells.iter().enumerate() {
            s += &format!(
                "        Item {{ x: {}; y: {}; width: {}; height: {}\n            {qml}\n        }}\n",
                k % self.cols * self.cell.0,
                k / self.cols * self.cell.1,
                self.cell.0,
                self.cell.1
            );
        }
        s + "    }\n"
    }

    /// The scene before and after `step()` as Qt draws it.
    fn qt(&self) -> [Canvas; 2] {
        let dir = tempfile::tempdir().unwrap();
        let at = |n: &str| dir.path().join(n).to_str().unwrap().to_string();
        let (w, h) = self.size();
        // The first frame on screen starts it; each picture is taken when the one before is
        // saved.
        let source = format!(
            "import QtQuick\nimport QtQuick.Effects\nimport Qt5Compat.GraphicalEffects\n\
             Window {{\n    width: {w}; height: {h}; visible: true\n    color: \"{BG}\"\n{}{}    \
             property bool asked: false\n    onFrameSwapped: {{\n        if (asked)\n            \
             return;\n        asked = true;\n        console.log(\"API\", GraphicsInfo.api, \
             Screen.devicePixelRatio);\n        scene.grabToImage(function(a) {{\n            \
             a.saveToFile(\"{}\");\n            scene.step();\n            \
             scene.grabToImage(function(b) {{\n                b.saveToFile(\"{}\");\n                \
             Qt.quit();\n            }});\n        }});\n    }}\n}}\n",
            self.types,
            self.body(),
            at("0.png"),
            at("1.png")
        );
        std::fs::write(at("scene.qml"), source).unwrap();
        let out = std::process::Command::new("qml6")
            .arg(at("scene.qml"))
            .env("QT_QPA_PLATFORM", "wayland-egl")
            .env("LIBGL_ALWAYS_SOFTWARE", "1")
            .env("QT_FORCE_STDERR_LOGGING", "1")
            // QGfxShaderBuilder writes the shaders it bakes for GaussianBlur into the cache
            // directory in place: a second Qt started meanwhile loads the unfinished file and
            // draws no blur.
            .env("XDG_CACHE_HOME", dir.path())
            .output()
            .unwrap();
        let log = String::from_utf8_lossy(&out.stderr);
        // QSGRendererInterface::OpenGL at scale 1: the software backend draws no effects.
        assert!(log.contains("API 3 1"), "qml6 printed:\n{log}");
        ["0.png", "1.png"].map(|n| {
            let c = qsg::image::load_png(&at(n)).unwrap_or_else(|e| panic!("{n}: {e}\n{log}"));
            assert_eq!((c.w, c.h), (w, h), "size of Qt's picture");
            c
        })
    }

    /// The same two pictures on our CPU or GPU path.
    fn ours(&self, gpu: bool) -> [Canvas; 2] {
        let (w, h) = self.size();
        let source = format!(
            "import QtQuick\nimport QtQuick.Effects\nimport Qt5Compat.GraphicalEffects\n\
             import Rustshell\nFloatingWindow {{\n    width: {w}; height: {h}\n    \
             color: \"{BG}\"\n    function step() {{ scene.step() }}\n{}{}}}\n",
            self.types,
            self.body()
        );
        // From a file: a document loaded from a string does not find its inline components.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("scene.qml");
        std::fs::write(&file, source).unwrap();
        let mut rt = qml::runtime::Runtime::new(Some(dir.path().to_path_buf()));
        let win = rt.load_file(&file).expect("scene loads");
        let mut fonts = scene::Fonts::default();
        let mut gl = gpu.then(|| {
            let (g, _) = shell::gpu::Gpu::headless("surfaceless")
                .expect("surfaceless EGL/GLES2 context (Mesa)");
            let s = g.pbuffer(w as i32, h as i32).expect("pbuffer");
            (g, s)
        });
        let mut picture = |rt: &mut qml::runtime::Runtime| {
            for _ in 0..3 {
                rt.settle();
                scene::layout_window(rt, &mut fonts, win, [w as f64, h as f64]);
            }
            invalidate(rt);
            match &mut gl {
                None => {
                    let mut c = Canvas::new(w, h, [0, 0, 0, 0]);
                    scene::paint_window(rt, &mut fonts, win, &mut Target::Cpu(&mut c));
                    c
                }
                Some((g, s)) => {
                    let mut fr = shell::gpu::Frame::new(w, h);
                    scene::paint_window(rt, &mut fonts, win, &mut Target::Gpu(&mut fr));
                    s.reswap();
                    g.render(s, &mut fr, || {});
                    g.read_surface(s)
                }
            }
        };
        let before = picture(&mut rt);
        rt.call_method(win, "step", Vec::new())
            .expect("step()")
            .expect("step() runs");
        [before, picture(&mut rt)]
    }

    /// Every cell of both pictures is within `tol` per channel of Qt's, and no more than
    /// `most` pixels of the two differ from Qt's at all: what was measured when the CPU
    /// raster took llvmpipe's linear filter and its way of writing a pixel.
    fn like_qt(&self, gpu: bool, tol: u8, most: usize) {
        if !testenv::require("qml6") {
            return;
        }
        let (qt, ours) = (self.qt(), self.ours(gpu));
        let (w, _) = self.size();
        let mut wrong = Vec::new();
        let pairs = qt.iter().zip(&ours);
        let differ: usize = pairs
            .map(|(qt, ours)| qt.px.iter().zip(&ours.px).filter(|(a, b)| a != b).count())
            .sum();
        println!("{differ} pixels differ from Qt's");
        if differ > most {
            wrong.push(format!("{differ} pixels differ from Qt's, {most} did"));
        }
        for (pass, (qt, ours)) in qt.iter().zip(&ours).enumerate() {
            for (k, (name, _)) in self.cells.iter().enumerate() {
                let (x0, y0) = (k % self.cols * self.cell.0, k / self.cols * self.cell.1);
                let cell =
                    (0..self.cell.1).flat_map(|y| (0..self.cell.0).map(move |x| (x0 + x, y0 + y)));
                let diff = |(x, y): (usize, usize)| {
                    let (a, b) = (ours.px[y * w + x], qt.px[y * w + x]);
                    (0..4).map(|c| a[c].abs_diff(b[c])).max().unwrap()
                };
                let (worst, (x, y)) = cell.clone().map(|p| (diff(p), p)).max().unwrap();
                let over = cell.filter(|&p| diff(p) > tol).count();
                if worst > tol {
                    wrong.push(format!(
                        "{name}, picture {pass}: {over} pixels differ by more than {tol}, the \
                         worst by {worst} at {},{} of the cell: {:?}, Qt has {:?}",
                        x - x0,
                        y - y0,
                        ours.px[y * w + x],
                        qt.px[y * w + x]
                    ));
                }
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }
}

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

/// Each effect with the properties the original's QML leaves unset; nothing changes between the pictures.
const DEFAULTS: Scene = Scene {
    cell: (100, 90),
    cols: 3,
    types: "",
    head: "        Item {
            id: src; width: 30; height: 24; visible: false
            Rectangle { anchors.fill: parent; radius: 8; color: \"#cc3366ff\" }
            Rectangle { x: 5; y: 4; width: 10; height: 9; color: \"#ffee8800\" }
        }
        Rectangle { id: mask; width: 30; height: 24; radius: 12; visible: false; layer.enabled: true }\n",
    cells: &[
        // ReloadPopup.qml's shadow.
        (
            "DropShadow without transparentBorder",
            "DropShadow { x: 35; y: 33; width: 30; height: 24; source: src; radius: 6; samples: 13
                color: \"#c0ffffff\"; verticalOffset: 2 }",
        ),
        (
            "DropShadow with only a source",
            "DropShadow { x: 35; y: 33; width: 30; height: 24; source: src }",
        ),
        (
            "GaussianBlur with only a source",
            "GaussianBlur { x: 35; y: 33; width: 30; height: 24; source: src }",
        ),
        // StyledRectangularShadow.qml's shadow, uncached and cached.
        (
            "RectangularShadow with a radius",
            "RectangularShadow { x: 35; y: 33; width: 30; height: 24; radius: 8; blur: 3; spread: 1
                offset: Qt.vector2d(0, 1); color: \"#ffffff\" }",
        ),
        (
            "cached RectangularShadow with a radius",
            "RectangularShadow { x: 35; y: 33; width: 30; height: 24; radius: 8; blur: 3; spread: 1
                offset: Qt.vector2d(0, 1); color: \"#ffffff\"; cached: true }",
        ),
        (
            "RectangularShadow with only a size",
            "RectangularShadow { x: 35; y: 33; width: 30; height: 24 }",
        ),
        (
            "MultiEffect blur",
            "MultiEffect { x: 35; y: 33; width: 30; height: 24; source: src; blurEnabled: true
                blur: 1 }",
        ),
        // autoPaddingEnabled stretches the mask over the padded effect.
        (
            "MultiEffect blur with a mask",
            "MultiEffect { x: 35; y: 33; width: 30; height: 24; source: src; blurEnabled: true
                blur: 1; blurMax: 8; maskEnabled: true; maskSource: mask }",
        ),
        (
            "MultiEffect colorization",
            "MultiEffect { x: 35; y: 33; width: 30; height: 24; source: src; colorization: 1 }",
        ),
        (
            "MultiEffect with only a source",
            "MultiEffect { x: 35; y: 33; width: 30; height: 24; source: src }",
        ),
    ],
    step: "",
};

/// MultiEffect's shadow: the source's alpha (through the blur chain, at the shadow's offset and
/// scale) tinted with shadowColor under the source. The last cells change between the pictures.
const SHADOW: Scene = Scene {
    cell: (140, 130),
    cols: 4,
    types: "    component Shadowed: MultiEffect {
        x: 55; y: 53; width: 30; height: 24; source: src; shadowEnabled: true
    }\n",
    head: "        Item {
            id: src; width: 30; height: 24; visible: false
            Rectangle { anchors.fill: parent; radius: 8; color: \"#cc3366ff\" }
            Rectangle { x: 5; y: 4; width: 10; height: 9; color: \"#ffee8800\" }
        }
        Rectangle {
            id: mask; width: 30; height: 24; radius: 12; visible: false; layer.enabled: true
        }
        Rectangle {
            id: smoothMask; width: 30; height: 24; radius: 12; visible: false
            layer.enabled: true; layer.smooth: true
        }\n",
    cells: &[
        ("only shadowEnabled", "Shadowed {}"),
        (
            "offset and a sharper shadow",
            "Shadowed { shadowHorizontalOffset: 6; shadowVerticalOffset: 4; shadowBlur: 0.3 }",
        ),
        (
            "translucent colour and opacity",
            "Shadowed { shadowColor: \"#80ff4020\"; shadowOpacity: 0.7; shadowVerticalOffset: 5
                shadowBlur: 0.5 }",
        ),
        (
            "larger shadow",
            "Shadowed { shadowScale: 1.4; shadowBlur: 0.2; shadowColor: \"#ffffff\" }",
        ),
        (
            "smaller shadow, offset",
            "Shadowed { shadowScale: 0.7; shadowBlur: 0; shadowHorizontalOffset: 20
                shadowColor: \"#ffffff\" }",
        ),
        (
            "with the effect's own blur",
            "Shadowed { blurEnabled: true; blur: 0.5; shadowHorizontalOffset: -5
                shadowVerticalOffset: 3; shadowColor: \"#00ff80\" }",
        ),
        (
            "with a mask",
            "Shadowed { blurMax: 8; maskEnabled: true; maskSource: mask; shadowVerticalOffset: 3
                shadowColor: \"#ffffff\" }",
        ),
        (
            "with a smooth mask",
            "Shadowed { blurMax: 8; maskEnabled: true; maskSource: smoothMask
                shadowVerticalOffset: 3; shadowColor: \"#ffffff\" }",
        ),
        (
            "with colorization",
            "Shadowed { colorization: 1; brightness: 0.2; shadowHorizontalOffset: 4
                shadowVerticalOffset: 4; shadowBlur: 0.1; shadowColor: \"#ffffff\" }",
        ),
        // No blur items: the source's own alpha, inside the unpadded effect.
        (
            "blurMax 0",
            "Shadowed { blurMax: 0; shadowHorizontalOffset: 4; shadowVerticalOffset: 3
                shadowColor: \"#ffffff\" }",
        ),
        (
            "blurMax 16",
            "Shadowed { blurMax: 16; shadowHorizontalOffset: 3; shadowColor: \"#ffffff\" }",
        ),
        (
            "blurMax 48",
            "Shadowed { blurMax: 48; shadowBlur: 0.8; shadowVerticalOffset: 6 }",
        ),
        (
            "blurMultiplier",
            "Shadowed { blurMax: 16; blurMultiplier: 1; shadowColor: \"#ffffff\" }",
        ),
        (
            "no autoPadding",
            "Shadowed { autoPaddingEnabled: false; shadowHorizontalOffset: 4
                shadowVerticalOffset: 3; shadowColor: \"#ffffff\" }",
        ),
        (
            "as a layer effect",
            "Rectangle { x: 55; y: 53; width: 30; height: 24; radius: 6; color: \"#e04040\"
                layer.enabled: true
                layer.effect: MultiEffect { shadowEnabled: true; shadowVerticalOffset: 4
                    shadowBlur: 0.6; shadowColor: \"#ffffff\" } }",
        ),
        (
            "shadowEnabled false",
            "Shadowed { shadowEnabled: false; shadowHorizontalOffset: 6 }",
        ),
        (
            "shadowEnabled, then every shadow property",
            "Shadowed { id: all }",
        ),
        (
            "no shadow, then shadowEnabled",
            "Shadowed { id: late; shadowEnabled: false; shadowVerticalOffset: 5 }",
        ),
        (
            "shadowEnabled, then none",
            "Shadowed { id: gone; shadowVerticalOffset: 5 }",
        ),
        (
            "blur, then a shadow as well",
            "Shadowed { id: both; shadowEnabled: false; blurEnabled: true; blur: 0.4; blurMax: 16
                shadowHorizontalOffset: 5; shadowColor: \"#ffffff\" }",
        ),
        (
            "blurMax 0, then 32",
            "Shadowed { id: grown; blurMax: 0; shadowVerticalOffset: 4; shadowColor: \"#ffffff\" }",
        ),
    ],
    step: "all.shadowHorizontalOffset = -6; all.shadowVerticalOffset = 7; all.shadowBlur = 0.4; \
           all.shadowOpacity = 0.5; all.shadowColor = \"#40c0ff\"; all.shadowScale = 1.2; \
           late.shadowEnabled = true; gone.shadowEnabled = false; both.shadowEnabled = true; \
           grown.blurMax = 32",
};

/// RectangularShadow's `antialiasing` through its life; the first two cells set it themselves.
const ANTIALIASING: Scene = Scene {
    cell: (50, 50),
    cols: 4,
    types: "    component Shadow: RectangularShadow {
        x: 10; y: 13; width: 30; height: 24; blur: 2; color: \"#ffffff\"
    }\n",
    head: "",
    cells: &[
        (
            "antialiasing: true",
            "Shadow { radius: 8; antialiasing: true }",
        ),
        (
            "antialiasing: false",
            "Shadow { radius: 8; antialiasing: false }",
        ),
        ("radius 8", "Shadow { radius: 8 }"),
        ("no radius", "Shadow {}"),
        ("radius 0", "Shadow { radius: 0 }"),
        ("radius 8, then 0", "Shadow { id: to0; radius: 8 }"),
        ("no radius, then 8", "Shadow { id: to8 }"),
        ("radius 8, then 4", "Shadow { id: to4; radius: 8 }"),
        (
            "antialiasing: true, radius 8, then 0",
            "Shadow { id: kept; radius: 8; antialiasing: true }",
        ),
        (
            "radius 8, then antialiasing = false",
            "Shadow { id: stale; radius: 8 }",
        ),
        (
            "radius 8, then antialiasing = false and blur 3",
            "Shadow { id: fresh; radius: 8 }",
        ),
        (
            "radius 8, then antialiasing = false, blur 3 and blur 2",
            "Shadow { id: back; radius: 8 }",
        ),
        (
            "radius 8, then antialiasing = false and x + 1",
            "Shadow { id: moved; radius: 8 }",
        ),
        (
            "antialiasing: false, then undefined",
            "Shadow { id: unset; radius: 8; antialiasing: false }",
        ),
        (
            "antialiasing: false, then undefined and blur 3",
            "Shadow { id: reset; radius: 8; antialiasing: false }",
        ),
        ("no radius, then 8 and 0", "Shadow { id: there }"),
        (
            "radius 8, 0 in Component.onCompleted",
            "Shadow { radius: 8; Component.onCompleted: radius = 0 }",
        ),
        (
            "radius 8 and no width, then radius 0 and width 30",
            "Shadow { id: late; radius: 8; width: 0 }",
        ),
    ],
    step: "to0.radius = 0; to8.radius = 8; to4.radius = 4; kept.radius = 0; \
           stale.antialiasing = false; fresh.antialiasing = false; fresh.blur = 3; \
           back.antialiasing = false; back.blur = 3; back.blur = 2; \
           moved.antialiasing = false; moved.x += 1; unset.antialiasing = undefined; \
           reset.antialiasing = undefined; reset.blur = 3; there.radius = 8; there.radius = 0; \
           late.radius = 0; late.width = 30",
};

/// RectangularShadow's corner radii, and shadows that get their window or their size late.
const CORNERS: Scene = Scene {
    cell: (50, 50),
    cols: 5,
    types: "    component Shadow: RectangularShadow {
        x: 10; y: 13; width: 30; height: 24; blur: 2; color: \"#ffffff\"
    }\n",
    head: "        Component { id: maker; Shadow {} }\n",
    cells: &[
        (
            "four radii",
            "Shadow { topLeftRadius: 2; topRightRadius: 6; bottomLeftRadius: 10
                bottomRightRadius: 14 }",
        ),
        (
            "four radii, cached",
            "Shadow { topLeftRadius: 2; topRightRadius: 6; bottomLeftRadius: 10
                bottomRightRadius: 14; cached: true }",
        ),
        (
            "radius 8, top left 1",
            "Shadow { radius: 8; topLeftRadius: 1 }",
        ),
        (
            "radius 8, bottom right 0",
            "Shadow { radius: 8; bottomRightRadius: 0 }",
        ),
        (
            "radius 8, top right -4",
            "Shadow { radius: 8; topRightRadius: -4 }",
        ),
        ("bottom left 9 alone", "Shadow { bottomLeftRadius: 9 }"),
        ("bottom left 0 alone", "Shadow { bottomLeftRadius: 0 }"),
        (
            "spread 3, blur 4",
            "Shadow { topLeftRadius: 10; bottomRightRadius: 3; spread: 3; blur: 4 }",
        ),
        (
            "spread 3, blur 4, cached",
            "Shadow { topLeftRadius: 10; bottomRightRadius: 3; spread: 3; blur: 4; cached: true }",
        ),
        (
            "spread 4 over radius 2",
            "Shadow { radius: 2; bottomLeftRadius: 9; spread: 4; blur: 1 }",
        ),
        (
            "blur 0 without antialiasing",
            "Shadow { topRightRadius: 9; blur: 0; antialiasing: false }",
        ),
        (
            "blur 0 without antialiasing or a radius",
            "Shadow { blur: 0; antialiasing: false }",
        ),
        (
            "blur 0 without antialiasing or a radius, cached",
            "Shadow { blur: 0; antialiasing: false; cached: true }",
        ),
        (
            "offset",
            "Shadow { topLeftRadius: 9; bottomRightRadius: 5; offset: Qt.vector2d(3, -2) }",
        ),
        (
            "radius 3, then top left 9",
            "Shadow { id: later; radius: 3 }",
        ),
        (
            "radius 8 and top left 0, then undefined",
            "Shadow { id: gone; radius: 8; topLeftRadius: 0 }",
        ),
        (
            "top left 9, then 0",
            "Shadow { id: zeroed; topLeftRadius: 9 }",
        ),
        (
            "bottom right 2, then 11",
            "Shadow { id: grows; bottomRightRadius: 2 }",
        ),
        (
            "cached radius 4, then bottom left 10",
            "Shadow { id: cachedLater; radius: 4; cached: true }",
        ),
        (
            "top left 2 and radius 3, then radius 9",
            "Shadow { id: under; topLeftRadius: 2; radius: 3 }",
        ),
        (
            "top right 9, then spread 3 and blur 4",
            "Shadow { id: spreads; topRightRadius: 9 }",
        ),
        (
            "made without a window, then parented",
            "Item { id: slotA; anchors.fill: parent }",
        ),
        (
            "made without a window, radius 6 and 0, then parented",
            "Item { id: slotB; anchors.fill: parent }",
        ),
        (
            "made empty without a window, parented, top right 9, then sized",
            "Item { id: slotC; anchors.fill: parent }",
        ),
        (
            "made in the window, then bottom left 8",
            "Item { id: slotD; anchors.fill: parent }",
        ),
        (
            "made without a window, radius 6 and 0, parented, then moved to another parent",
            "Item { id: slotE; anchors.fill: parent }",
        ),
    ],
    step: "later.topLeftRadius = 9; gone.topLeftRadius = undefined; zeroed.topLeftRadius = 0; \
           grows.bottomRightRadius = 11; cachedLater.bottomLeftRadius = 10; under.radius = 9; \
           spreads.spread = 3; spreads.blur = 4; \
           var a = maker.createObject(null); a.parent = slotA; \
           var b = maker.createObject(null); b.radius = 6; b.radius = 0; b.parent = slotB; \
           var c = maker.createObject(null, { width: 0 }); c.parent = slotC; \
           c.topRightRadius = 9; c.width = 30; \
           var d = maker.createObject(slotD); d.bottomLeftRadius = 8; \
           var e = maker.createObject(null); e.radius = 6; e.radius = 0; e.parent = slotA; \
           e.parent = slotE",
};

#[test]
fn unset_effect_properties_paint_like_qt_on_the_cpu() {
    DEFAULTS.like_qt(false, TOL, 52);
}

#[test]
fn unset_effect_properties_paint_like_qt_on_the_gpu() {
    DEFAULTS.like_qt(true, TOL, usize::MAX);
}

#[test]
fn multieffect_shadow_paints_like_qt_on_the_cpu() {
    SHADOW.like_qt(false, TOL, 183);
}

#[test]
fn multieffect_shadow_paints_like_qt_on_the_gpu() {
    SHADOW.like_qt(true, TOL, usize::MAX);
}

#[test]
fn rectangular_shadow_antialiasing_follows_qt_on_the_cpu() {
    ANTIALIASING.like_qt(false, TOL, 0);
}

#[test]
fn rectangular_shadow_antialiasing_follows_qt_on_the_gpu() {
    ANTIALIASING.like_qt(true, TOL, usize::MAX);
}

#[test]
fn rectangular_shadow_corners_and_late_windows_paint_like_qt_on_the_cpu() {
    CORNERS.like_qt(false, TOL, 0);
}

#[test]
fn rectangular_shadow_corners_and_late_windows_paint_like_qt_on_the_gpu() {
    CORNERS.like_qt(true, TOL, usize::MAX);
}

/// RectangularShadows whose properties QML reads: `state()` lists `antialiasing`, `radius`
/// and the four corner radii of the declared ones, `heard` collects their change signals.
const VALUES: &str = r#"
    id: root; width: 200; height: 200
    property string log
    property string heard
    function say(what) { log += what + "\n" }
    function told() { say("  heard " + heard); heard = "" }
    component Shadow: RectangularShadow { width: 40; height: 40 }
    // Declared after its target, it completes and so connects before it: Qt's signal
    // handlers of the shadow itself would also hear the values set while it is created.
    component Ear: Connections {
        function onAntialiasingChanged() { root.heard += target.objectName + "=" + target.antialiasing + " " }
        function onTopLeftRadiusChanged() { root.heard += target.objectName + ".tl=" + target.topLeftRadius + " " }
        function onRadiusChanged() { root.heard += target.objectName + ".r=" + target.radius + " " }
        function onBlurChanged() { root.heard += target.objectName + ".b=" + target.blur + " " }
    }
    Shadow { id: plain; objectName: "plain" }
    Shadow { id: round; objectName: "round"; radius: 5 }
    Shadow { id: off; objectName: "off"; radius: 5; antialiasing: false }
    Shadow { id: on; objectName: "on"; antialiasing: true }
    Shadow { id: empty; objectName: "empty"; width: 0; height: 0 }
    Shadow { id: corner; objectName: "corner"; topLeftRadius: 5 }
    Shadow { id: sharp; objectName: "sharp"; radius: 5; topLeftRadius: 0; topRightRadius: 0 }
    Shadow { id: below; objectName: "below"; bottomRightRadius: -3 }
    Shadow { id: zero; objectName: "zero"; bottomLeftRadius: 0 }
    Ear { target: plain } Ear { target: round } Ear { target: off } Ear { target: on }
    Ear { target: empty } Ear { target: corner } Ear { target: sharp } Ear { target: below }
    Ear { target: zero }
    property bool bound: plain.antialiasing
    property real boundCorner: round.topLeftRadius
    Component {
        id: maker
        Shadow {
            id: made
            objectName: "made"
            onAntialiasingChanged: root.heard += "made=" + antialiasing + " "
            onRadiusChanged: root.heard += "made.r=" + radius + " "
        }
    }
    property var made
    function read(s) {
        return s.antialiasing + "/" + [s.radius, s.topLeftRadius, s.topRightRadius,
                                       s.bottomLeftRadius, s.bottomRightRadius] + "/" + s.blur
    }
    function state() {
        return [plain, round, off, on, empty, corner, sharp, below, zero].map(read).join(" ")
                + " bound " + bound + " " + boundCorner
    }
    Component.onCompleted: {
        say("completed " + state()); told()
        made = maker.createObject(null); say("made " + read(made))
        made.radius = 3; say("made radius 3 " + read(made))
        made.radius = 0; say("made radius 0 " + read(made))
        made.topRightRadius = 2; say("made topRightRadius 2 " + read(made))
        made.topRightRadius = undefined; say("made topRightRadius undefined " + read(made))
        made.parent = root; say("made parent " + read(made))
        made.width = 41; say("made width " + read(made))
        told()
    }
    function step() {
        say("shown " + state() + " made " + read(made)); told()
        plain.radius = 4; say("plain radius 4 " + read(plain))
        plain.radius = 0; say("plain radius 0 " + read(plain))
        plain.radius = -2; say("plain radius -2 " + read(plain))
        plain.blur = -3; say("plain blur -3 " + read(plain))
        plain.blur = 4; plain.blur = -1; say("plain blur -1 " + read(plain))
        plain.topLeftRadius = 2; say("plain topLeftRadius 2 " + read(plain))
        plain.topLeftRadius = 0; say("plain topLeftRadius 0 " + read(plain))
        plain.topLeftRadius = undefined; say("plain topLeftRadius undefined " + read(plain))
        plain.antialiasing = true; say("plain antialiasing true " + read(plain))
        plain.antialiasing = undefined; say("plain antialiasing undefined " + read(plain))
        told()
        round.radius = 7; say("round radius 7 " + read(round) + " bound " + boundCorner)
        round.topLeftRadius = 1; say("round topLeftRadius 1 " + read(round) + " bound " + boundCorner)
        round.topLeftRadius = 1; round.topLeftRadius = undefined
        say("round topLeftRadius undefined " + read(round) + " bound " + boundCorner)
        round.topLeftRadius = undefined
        told()
        off.antialiasing = undefined; say("off antialiasing undefined " + read(off))
        off.radius = 0; say("off radius 0 " + read(off))
        on.antialiasing = false; on.radius = 3; say("on false, radius 3 " + read(on))
        on.antialiasing = undefined; say("on antialiasing undefined " + read(on))
        told()
        empty.width = 10; say("empty width " + read(empty))
        empty.height = 10; say("empty height " + read(empty))
        corner.topLeftRadius = 0; say("corner topLeftRadius 0 " + read(corner))
        below.radius = 1; say("below radius 1 " + read(below))
        zero.bottomLeftRadius = 3; zero.bottomLeftRadius = -1; say("zero -1 " + read(zero))
        told()
        var late = maker.createObject(null); late.radius = 0; say("late " + read(late))
        late.parent = root; say("late parent " + read(late))
        late.radius = 0; say("late radius 0 " + read(late))
        var here = maker.createObject(root); say("here " + read(here))
        var held = maker.createObject(null, { width: 0 }); held.parent = root
        say("held parent " + read(held))
        held.width = 5; say("held width " + read(held))
        made.parent = null; say("made without parent " + read(made))
        told()
    }
"#;

/// What `VALUES` logs in Qt and in our runtime, with the shadows under a root Item, which
/// gets its window after completion, or inside a window from the start.
fn value_logs(in_window: bool) -> [String; 2] {
    let dir = tempfile::tempdir().unwrap();
    let at = |n: &str| dir.path().join(n);
    let imports = "import QtQuick\nimport QtQuick.Effects\n";
    // Qt runs step() at the first frame on screen and prints the log.
    let frame = "property bool stepped: false\n    function frame() {\n        if (stepped)\n            \
                 return;\n        stepped = true;\n        root.step();\n        \
                 console.log(\"LOG\\n\" + root.log + \"END\");\n        Qt.quit();\n    }\n";
    let (qt, ours) = if in_window {
        (
            format!(
                "{imports}Window {{\n    width: 200; height: 200; visible: true\n    {frame}    \
                 onFrameSwapped: frame()\n    Item {{{VALUES}}}\n}}\n"
            ),
            format!(
                "{imports}import Rustshell\nFloatingWindow {{\n    width: 200; height: 200\n    \
                 property alias log: root.log\n    function step() {{ root.step() }}\n    \
                 Item {{{VALUES}}}\n}}\n"
            ),
        )
    } else {
        (
            format!(
                "{imports}Item {{{VALUES}    {frame}    Connections {{\n        \
                 target: root.Window.window\n        function onFrameSwapped() {{ root.frame() }}\n    \
                 }}\n}}\n"
            ),
            format!("{imports}Item {{{VALUES}}}\n"),
        )
    };
    std::fs::write(at("qt.qml"), qt).unwrap();
    std::fs::write(at("ours.qml"), ours).unwrap();
    let out = std::process::Command::new("qml6")
        .arg(at("qt.qml"))
        .env("QT_QPA_PLATFORM", "wayland-egl")
        .env("LIBGL_ALWAYS_SOFTWARE", "1")
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .env("XDG_CACHE_HOME", dir.path())
        .output()
        .unwrap();
    let printed = String::from_utf8_lossy(&out.stderr);
    let qt = printed
        .split_once("LOG\n")
        .and_then(|(_, l)| l.split_once("END"))
        .unwrap_or_else(|| panic!("qml6 printed:\n{printed}"))
        .0
        .to_string();
    let mut rt = qml::runtime::Runtime::new(Some(dir.path().to_path_buf()));
    let win = rt.load_file(&at("ours.qml")).expect("scene loads");
    let mut fonts = scene::Fonts::default();
    rt.settle();
    // The shell shows the root: an Item is in a window from here on.
    scene::layout_window(&mut rt, &mut fonts, win, [200.0, 200.0]);
    rt.settle();
    rt.call_method(win, "step", Vec::new())
        .expect("step()")
        .expect("step() runs");
    let ours = match rt.world.borrow_mut().read(win, "log") {
        Some(qml::value::Value::Str(s)) => s.to_string(),
        v => panic!("log is {v:?}"),
    };
    [qt, ours]
}

#[test]
fn rectangular_shadow_properties_read_like_qt_before_and_in_a_window() {
    if !testenv::require("qml6") {
        return;
    }
    let [qt, ours] = value_logs(false);
    assert_eq!(ours, qt, "ours, then Qt's");
}

#[test]
fn rectangular_shadow_properties_read_like_qt_inside_a_window() {
    if !testenv::require("qml6") {
        return;
    }
    let [qt, ours] = value_logs(true);
    assert_eq!(ours, qt, "ours, then Qt's");
}
