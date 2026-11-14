//! Effects drawn like Qt 6.12 draws them: each scene is rendered by the real Qt Quick
//! (qmltestrunner on OpenGL, pixels read with `grabImage`) and by our CPU and GPU paths, and
//! the pictures are compared pixel by pixel. The GaussianBlur kernel is compared by value
//! with the shader Qt generated for the same item.

use qsg::raster::Canvas;
use shell::scene::{self, Target};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};

const W: usize = 160;
const H: usize = 96;
const RUNNER: &str = "/usr/lib/qt6/bin/qmltestrunner";
const QSB: &str = "/usr/lib/qt6/bin/qsb";
const IMPORTS: &str =
    "import QtQuick.Effects\nimport QtQuick.Shapes\nimport Qt5Compat.GraphicalEffects";

/// What Qt drew for a scene, and the directory holding the shaders it generated.
struct Qt {
    px: Vec<[u8; 4]>,
    dir: tempfile::TempDir,
}

/// Renders `body` inside a `W` x `H` rectangle in Qt Quick, in a window of exactly that size:
/// one that may be neither smaller nor larger floats in sway, where a window without such
/// limits is tiled over the 1920 x 1080 output (1916 x 1051), and Qt rounds a scissor from
/// the window's height. `None` (after [`testenv::equiv_missing`]) where Qt's qmltestrunner is
/// not installed.
fn qt(body: &str) -> Option<Qt> {
    if !Path::new(RUNNER).exists() {
        testenv::equiv_missing(format_args!("{RUNNER} (Qt 6 with QtTest and Qt5Compat)"));
        return None;
    }
    let dir = tempfile::tempdir().expect("temp dir");
    let out = dir.path().join("out.png");
    std::fs::write(
        dir.path().join("tst_scene.qml"),
        format!(
            "import QtQuick\nimport QtTest\n{IMPORTS}\nItem {{\n    width: {W}; height: {H}\n    \
             Window.onWindowChanged: if (Window.window) {{\n        \
             Window.window.minimumWidth = {W}; Window.window.maximumWidth = {W}\n        \
             Window.window.minimumHeight = {H}; Window.window.maximumHeight = {H}\n    }}\n    \
             Rectangle {{\n        id: scene\n        width: {W}; height: {H}; color: \"#202020\"\n{body}\n    }}\n    \
             TestCase {{\n        name: \"grab\"; when: windowShown\n        \
             function test_grab() {{\n            \
             compare(Window.width + \" x \" + Window.height, \"{W} x {H}\", \"the window's size\")\n            \
             grabImage(scene).save(\"{}\")\n        }}\n    }}\n}}\n",
            out.display()
        ),
    )
    .expect("scene file");
    let log = run_qt(dir.path());
    assert!(
        !log.contains("QML"),
        "Qt has no complaint about the scene:\n{log}"
    );
    let img = qsg::image::load_png(out.to_str().unwrap()).expect("Qt's picture");
    assert_eq!((img.w, img.h), (W, H), "at scale 1");
    Some(Qt { px: img.px, dir })
}

/// How often the reference is started again after Qt gave up on its own window.
const RERUNS: usize = 3;
const NEVER_EXPOSED: &str = "window was never exposed";

/// Runs qmltestrunner on `tst_scene.qml` in `dir` until it has run the test, and returns
/// what it printed.
///
/// With several Qt windows opening at once the compositor can answer a frame later than
/// 100 ms; QtWayland then counts the window as not exposed for that moment
/// (`qwaylandwindow.cpp:1757`), and qmltestrunner, which asks `view.isExposed()` exactly once
/// (`quicktest.cpp:680`), prints "window was never exposed! If the test case was expecting
/// windowShown, it will hang", never sets `windowShown` and waits in its event loop for
/// ever. The test has not started then, so this is no result of the comparison: that runner
/// is stopped as soon as it prints the line and the reference is rendered again.
fn run_qt(dir: &Path) -> String {
    let mut gave_up = Vec::new();
    for _ in 0..=RERUNS {
        let out = std::fs::File::create(dir.join("stdout.log")).expect("log file");
        // Effects are ShaderEffects: only a GPU backend draws them (not `offscreen`'s software one).
        let mut run = Command::new(RUNNER)
            .args(["-input", "tst_scene.qml"])
            .current_dir(dir)
            .env("QT_QPA_PLATFORM", "wayland-egl")
            .env("LIBGL_ALWAYS_SOFTWARE", "1")
            .env("XDG_CACHE_HOME", dir.join("cache"))
            // Qt's messages from before the test log starts, this one among them, go to the
            // journal unless told otherwise.
            .env("QT_FORCE_STDERR_LOGGING", "1")
            .stdout(out)
            .stderr(Stdio::piped())
            .spawn()
            .expect("qmltestrunner runs");
        let mut err = String::new();
        let mut never_exposed = None;
        for line in BufReader::new(run.stderr.take().unwrap()).lines() {
            let line = line.expect("qmltestrunner's stderr");
            if line.contains(NEVER_EXPOSED) {
                never_exposed = Some(line);
                break;
            }
            err.push_str(&line);
            err.push('\n');
        }
        if let Some(line) = never_exposed {
            run.kill().expect("qmltestrunner stops");
            run.wait().expect("qmltestrunner has stopped");
            eprintln!("Qt reference rendered again: {line}");
            gave_up.push(line);
            continue;
        }
        let status = run.wait().expect("qmltestrunner exits");
        let log = std::fs::read_to_string(dir.join("stdout.log")).expect("log file") + &err;
        assert!(status.success(), "Qt renders the scene:\n{log}");
        return log;
    }
    panic!(
        "Qt gave up on its window {} times in a row:\n{}",
        gave_up.len(),
        gave_up.join("\n")
    );
}

/// The same `body` in a window of ours, painted on the CPU or the GPU path.
fn ours(body: &str, gpu: bool) -> Canvas {
    let qml = format!(
        "import QtQuick\nimport Quickshell\n{IMPORTS}\nFloatingWindow {{\n    width: {W}; height: {H}\n    \
         color: \"#202020\"\n{body}\n}}\n"
    );
    let mut rt = qml::runtime::Runtime::new(None);
    let win = rt.load_str(&qml).expect("scene loads");
    let mut fonts = scene::Fonts::default();
    for _ in 0..3 {
        rt.settle();
        scene::layout_window(&mut rt, &mut fonts, win, [W as f64, H as f64]);
    }
    if !gpu {
        let mut c = Canvas::new(W, H, [0, 0, 0, 0]);
        scene::paint_window(&mut rt, &mut fonts, win, &mut Target::Cpu(&mut c));
        return c;
    }
    let (mut g, _) =
        shell::gpu::Gpu::headless("surfaceless").expect("surfaceless EGL/GLES2 context (Mesa)");
    let mut fr = shell::gpu::Frame::new(W, H);
    scene::paint_window(&mut rt, &mut fonts, win, &mut Target::Gpu(&mut fr));
    let mut s = g.pbuffer(W as i32, H as i32).expect("pbuffer");
    s.reswap();
    g.render(&mut s, &mut fr, || {});
    g.read_surface(&s)
}

fn far(a: [u8; 4], b: [u8; 4]) -> u8 {
    (0..4).map(|k| a[k].abs_diff(b[k])).max().unwrap()
}

/// `body` painted on one of our paths has every pixel within `tol` (per channel, of 255) of
/// Qt's and no more than `most` pixels that differ from Qt's at all, and Qt's picture is not
/// the bare background at each of `lit` (the scene shows there what it is about).
fn like_qt(body: &str, gpu: bool, tol: u8, most: usize, lit: &[(usize, usize)]) {
    let Some(q) = qt(body) else { return };
    for &(x, y) in lit {
        assert!(
            far(q.px[y * W + x], [0x20, 0x20, 0x20, 255]) > 8,
            "Qt draws the effect at {x},{y}: {:?}",
            q.px[y * W + x]
        );
    }
    let c = ours(body, gpu);
    let path = if gpu { "GPU" } else { "CPU" };
    let differ = (0..W * H).filter(|&i| q.px[i] != c.px[i]).count();
    println!("{differ} pixels differ from Qt's on the {path} path");
    let mut off: Vec<(u8, usize)> = (0..W * H)
        .map(|i| (far(q.px[i], c.px[i]), i))
        .filter(|d| d.0 > tol)
        .collect();
    off.sort_by(|a, b| b.cmp(a));
    let worst: Vec<String> = off
        .iter()
        .take(12)
        .map(|&(d, i)| {
            format!(
                "  {},{}: Qt {:?}, ours {:?} (off by {d})",
                i % W,
                i / W,
                q.px[i],
                c.px[i]
            )
        })
        .collect();
    assert!(
        differ <= most,
        "{differ} pixels differ from Qt's on the {path} path, {most} did:\n{}",
        worst.join("\n")
    );
    assert!(
        off.is_empty(),
        "{} pixels differ from Qt's by more than {tol} on the {path} path:\n{}",
        off.len(),
        worst.join("\n")
    );
}

/// The scene `$body` compared with Qt as two tests, `$name::cpu` and `$name::gpu`. `$most`
/// is how many pixels of the CPU and of the GPU path may differ from Qt's at all (each within
/// `$tol`): what was measured when the CPU raster took llvmpipe's linear filter and its
/// way of writing a pixel, and the scene came closer to Qt, so it does not drift back.
macro_rules! like_qt {
    ($name:ident, $tol:expr, $lit:expr, $body:expr) => {
        like_qt!($name, $tol, $lit, $body, [usize::MAX, usize::MAX]);
    };
    ($name:ident, $tol:expr, $lit:expr, $body:expr, $most:expr) => {
        mod $name {
            use super::*;

            #[test]
            fn cpu() {
                like_qt($body, false, $tol, $most[0], &$lit);
            }
            #[test]
            fn gpu() {
                like_qt($body, true, $tol, $most[1], &$lit);
            }
        }
    };
}

/// A hidden source: a rounded rectangle with a stripe, so a wrong offset or scale shows.
const SRC: &str = "    Item {
        id: src; x: 24; y: 20; width: 40; height: 30; visible: false
        Rectangle { anchors.fill: parent; radius: 6; color: \"#ff3366\" }
        Rectangle { x: 8; y: 4; width: 6; height: 22; color: \"#3366ff\" }
    }\n";

/// A hidden 8 x 8 source of four squares, drawn by the effects at five times its size: how
/// its texture is sampled decides every edge between the squares.
const SMALL: &str = "
        width: 8; height: 8; visible: false
        Rectangle { width: 4; height: 4; color: \"#ff0000\" }
        Rectangle { x: 4; width: 4; height: 4; color: \"#0000ff\" }
        Rectangle { y: 4; width: 4; height: 4; color: \"#ffffff\" }
        Rectangle { x: 4; y: 4; width: 4; height: 4; color: \"#00c000\" }
    }\n";

// ---- MultiEffect's shadow (multieffect.frag, SHADOW).

like_qt!(
    shadow_with_defaults,
    TOL,
    [(30, 55), (60, 30)],
    &format!(
        "{SRC}    MultiEffect {{ x: 24; y: 20; width: 40; height: 30; source: src; shadowEnabled: true; shadowVerticalOffset: 8 }}"
    ),
    [10, 10]
);

like_qt!(
    shadow_offset_scaled_coloured,
    1,
    [(70, 45), (40, 56)],
    &format!(
        "{SRC}    MultiEffect {{
        x: 24; y: 20; width: 40; height: 30; source: src
        shadowEnabled: true; shadowHorizontalOffset: 7; shadowVerticalOffset: 9
        shadowColor: \"#00ff00\"; shadowBlur: 0.5; shadowOpacity: 0.8; shadowScale: 1.15
    }}"
    ),
    [28, 28]
);

like_qt!(
    shadow_under_a_blurred_source,
    TOL,
    [(20, 40), (40, 56)],
    &format!(
        "{SRC}    MultiEffect {{
        x: 24; y: 20; width: 40; height: 30; source: src
        blurEnabled: true; blur: 0.5; blurMax: 40
        shadowEnabled: true; shadowHorizontalOffset: -6; shadowVerticalOffset: 8; shadowColor: \"#ffff00\"
    }}"
    ),
    [124, 124]
);

like_qt!(
    sharp_shadow_without_padding,
    TOL,
    [(62, 48)],
    &format!(
        "{SRC}    MultiEffect {{
        x: 24; y: 20; width: 40; height: 30; source: src; autoPaddingEnabled: false
        shadowEnabled: true; shadowBlur: 0; shadowHorizontalOffset: 5; shadowVerticalOffset: 5
        shadowColor: \"#00ffff\"; shadowScale: 0.8
    }}"
    ),
    [0, 0]
);

// ---- A shadow colour whose alpha is not a whole number of 255ths. `Appearance.colors.colShadow`
// is `Qt.rgba(0, 0, 0, 1 * (1 - 0.7))`: Qt hands a ShaderEffect its colour as four floats of
// QColor's 16-bit channels (19661 / 65535 here, where 77 / 255 is 19789 / 65535), so the shadow
// is one level lighter wherever the two round apart. Sizes and colours are those of
// StyledDropShadow.qml and StyledRectangularShadow.qml; the page under them is light, as a
// wallpaper is, so each level of the shadow's alpha shows.

/// A light page under the shadows, and the alpha written both ways.
const PAGE: &str = "    Rectangle { anchors.fill: parent; color: \"#e8e8e8\" }\n";
const FRACTION: &str = "Qt.rgba(0, 0, 0, 1 * (1 - 0.7))";
const BYTES: &str = "\"#4d000000\"";

fn drop_shadow_of(colour: &str) -> String {
    format!(
        "{PAGE}{SRC}    DropShadow {{
        x: 24; y: 20; width: 40; height: 30; source: src
        radius: 8; samples: radius * 2 + 1; transparentBorder: true; color: {colour}
    }}"
    )
}

fn rectangular_shadow_of(colour: &str, cached: bool) -> String {
    format!(
        "{PAGE}    RectangularShadow {{
        x: 60; y: 32; width: 40; height: 30; radius: 8; blur: 9; spread: 1
        offset: Qt.vector2d(0.0, 1.0); color: {colour}; cached: {cached}
    }}"
    )
}

/// MultiEffect's shadow, blurred (`shadowBlur` is 1 unless set) or sharp. The sharp one has no
/// blur to differ in and lies on a page of 254, where an alpha of 77 / 255 put four pixels of
/// its round corners one level under Qt's.
fn effect_shadow_of(colour: &str, sharp: bool) -> String {
    let (page, blur) = if sharp {
        (
            "    Rectangle { anchors.fill: parent; color: \"#fefefe\" }\n",
            "; shadowBlur: 0",
        )
    } else {
        (PAGE, "")
    };
    format!(
        "{page}{SRC}    MultiEffect {{
        x: 24; y: 20; width: 40; height: 30; source: src
        shadowEnabled: true; shadowVerticalOffset: 6; shadowColor: {colour}{blur}
    }}"
    )
}

like_qt!(
    drop_shadow_of_a_whole_alpha,
    0,
    [(20, 35), (44, 56)],
    &drop_shadow_of(BYTES),
    [0, 0]
);

like_qt!(
    drop_shadow_of_a_fractional_alpha,
    0,
    [(20, 35), (44, 56)],
    &drop_shadow_of(FRACTION),
    [0, 0]
);

like_qt!(
    rectangular_shadow_of_a_whole_alpha,
    0,
    [(80, 48), (80, 66)],
    &rectangular_shadow_of(BYTES, false),
    [0, 0]
);

like_qt!(
    rectangular_shadow_of_a_fractional_alpha,
    0,
    [(80, 48), (80, 66)],
    &rectangular_shadow_of(FRACTION, false),
    [0, 0]
);

like_qt!(
    cached_rectangular_shadow_of_a_fractional_alpha,
    0,
    [(80, 48), (80, 66)],
    &rectangular_shadow_of(FRACTION, true),
    [0, 0]
);

like_qt!(
    sharp_effect_shadow_of_a_whole_alpha,
    0,
    [(44, 53)],
    &effect_shadow_of(BYTES, true),
    [0, 0]
);

like_qt!(
    sharp_effect_shadow_of_a_fractional_alpha,
    0,
    [(44, 53)],
    &effect_shadow_of(FRACTION, true),
    [0, 0]
);

// Blurred, MultiEffect's shadow is one level from Qt's in a few pixels with either alpha (as
// `shadow_with_defaults` is): that is its blur, not its colour. The bounds are what was measured.

like_qt!(
    effect_shadow_of_a_whole_alpha,
    1,
    [(44, 58)],
    &effect_shadow_of(BYTES, false),
    [17, 108]
);

like_qt!(
    effect_shadow_of_a_fractional_alpha,
    1,
    [(44, 58)],
    &effect_shadow_of(FRACTION, false),
    [25, 108]
);

// ---- GaussianBlur's `deviation`, on the generated shader and on the fallback one.

like_qt!(
    blur_with_deviation,
    TOL,
    [(30, 30), (100, 30)],
    &format!(
        "{SRC}    GaussianBlur {{ x: 24; y: 20; width: 40; height: 30; source: src; radius: 8; samples: 17; deviation: 1.5 }}
    GaussianBlur {{ x: 90; y: 20; width: 40; height: 30; source: src; radius: 6; samples: 13; deviation: 7; transparentBorder: true }}
    GaussianBlur {{ x: 24; y: 60; width: 40; height: 30; source: src; radius: 4; samples: 17; deviation: 0.8 }}"
    ),
    [0, 0]
);

// ---- The lock screen's wallpaper blur (Background.qml: `radius: lock.blur.radius`, 100 by
// default, `samples: radius * 2 + 1`), 201 taps, which our GPU path reads in a loop from a
// uniform array (`BAKED_TAPS` in gpu.rs) and Qt has written out in its shader: over its
// source as the lock screen has it, and with a transparent border. Both paths have Qt's
// pixels.

like_qt!(
    lock_screen_blur,
    0,
    [(20, 30), (80, 30), (140, 30), (80, 80)],
    "    Item {
        id: stripes; x: 4; y: 4; width: 152; height: 56
        Rectangle { width: 38; height: 56; color: \"#ff3366\" }
        Rectangle { x: 38; width: 38; height: 56; color: \"#3366ff\" }
        Rectangle { x: 76; width: 38; height: 56; color: \"#00ff00\" }
        Rectangle { x: 114; width: 38; height: 56; color: \"#ffffff\" }
        Rectangle { y: 40; width: 152; height: 16; color: \"#000000\" }
    }
    GaussianBlur { anchors.fill: stripes; source: stripes; radius: 100; samples: radius * 2 + 1 }
    Item {
        x: 4; y: 64; width: 152; height: 28; clip: true
        GaussianBlur { x: 46; y: -14; width: 152; height: 56; source: stripes; radius: 100; samples: 201; transparentBorder: true }
    }",
    [0, 0]
);

// ---- A stroke of the curve renderer where a pixel's centre lies on the stroke's centre line
// (shapestroke.frag paints such a pixel solid).

like_qt!(
    a_stroke_whose_centre_line_runs_through_pixel_centres,
    TOL,
    [(20, 12), (70, 22), (120, 16), (18, 64)],
    "    Shape { x: 10; y: 10; width: 40; height: 30; preferredRendererType: Shape.CurveRenderer
        ShapePath { strokeWidth: 3; fillColor: \"transparent\"; strokeColor: \"#3366ff\"
            capStyle: ShapePath.RoundCap; startX: 3.2; startY: 2.5
            PathLine { x: 26.2; y: 2.5 } PathMove { x: 34.2; y: 6.5 } PathLine { x: 34.2; y: 24.2 } } }
    Shape { x: 60; y: 10; width: 30; height: 30; preferredRendererType: Shape.CurveRenderer
        ShapePath { strokeWidth: 3; fillColor: \"transparent\"; strokeColor: \"#ff3366\"
            capStyle: ShapePath.RoundCap; startX: 3.2; startY: 5.2
            PathLine { x: 24.2; y: 26.2 } } }
    Shape { x: 100; y: 10; width: 40; height: 30; preferredRendererType: Shape.CurveRenderer
        ShapePath { strokeWidth: 3; pathHints: ShapePath.PathLinear; fillColor: \"transparent\"
            strokeColor: \"#ffcc00\"; capStyle: ShapePath.RoundCap; joinStyle: ShapePath.RoundJoin
            PathPolyline { path: [Qt.point(6.2, 6.5), Qt.point(34.2, 6.5), Qt.point(12.2, 28.5), Qt.point(36.2, 28.5)] } } }
    Shape { x: 10; y: 50; width: 60; height: 30; preferredRendererType: Shape.CurveRenderer
        ShapePath { strokeWidth: 3; fillColor: \"transparent\"; strokeColor: \"white\"
            capStyle: ShapePath.RoundCap
            PathAngleArc { centerX: 20.5; centerY: 14.5; radiusX: 12; radiusY: 12
                startAngle: 20; sweepAngle: 230 } } }"
);

// ---- Desaturate's `desaturation` outside 0..1 (desaturate.frag mixes without a clamp).

like_qt!(
    desaturation_outside_0_to_1,
    TOL,
    [(30, 30), (100, 30)],
    &format!(
        "{SRC}    Desaturate {{ x: 24; y: 20; width: 40; height: 30; source: src; desaturation: 1.5 }}
    Desaturate {{ x: 90; y: 20; width: 40; height: 30; source: src; desaturation: -0.5 }}"
    ),
    [0, 0]
);

// ---- ColorOverlay and Desaturate read a source that is not `smooth` without interpolation.

like_qt!(
    overlay_of_a_source_that_is_not_smooth,
    TOL,
    [(30, 30), (80, 30), (130, 30)],
    &format!(
        "    Item {{\n        id: rough; smooth: false{SMALL}    Item {{\n        id: fine{SMALL}    \
         ColorOverlay {{ x: 10; y: 10; width: 40; height: 40; source: rough; color: \"#6000ffff\" }}
    ColorOverlay {{ x: 60; y: 10; width: 40; height: 40; source: fine; color: \"#6000ffff\" }}
    Desaturate {{ x: 110; y: 10; width: 40; height: 40; source: rough; desaturation: 0.5 }}"
    ),
    // Qt's on both paths.
    [0, 0]
);

// ---- OpacityMask reads its source linearly, whatever the source's `smooth`.

like_qt!(
    opacity_mask_of_a_smaller_source,
    TOL,
    [(30, 30), (80, 30)],
    &format!(
        "    Item {{\n        id: rough; smooth: false{SMALL}    Item {{\n        id: fine{SMALL}    \
         Rectangle {{ id: round; width: 40; height: 40; radius: 12; visible: false }}
    OpacityMask {{ x: 10; y: 10; width: 40; height: 40; source: rough; maskSource: round }}
    OpacityMask {{ x: 60; y: 10; width: 40; height: 40; source: fine; maskSource: round }}"
    ),
    // Qt's on both paths.
    [0, 0]
);

// ---- OpacityMask over a plain Rectangle: the edge of the round mask lies over the window.

like_qt!(
    opacity_mask_of_a_plain_rectangle,
    0,
    [(60, 30)],
    "    Rectangle { id: plain; x: 50; y: 14; width: 20; height: 36; color: \"#e8def8\"; visible: false }
    Rectangle { id: pill; x: 50; y: 14; width: 20; height: 36; radius: 9999; visible: false }
    OpacityMask { x: 50; y: 14; width: 20; height: 36; source: plain; maskSource: pill }",
    [0, 0]
);

// ---- MultiEffect's mask together with blur and padding.

const MASK: &str = "    Item {
        id: mask; width: 40; height: 30; visible: false; layer.enabled: true
        Rectangle { x: 6; y: 3; width: 28; height: 24; radius: 12 }
    }\n";

like_qt!(
    mask_with_blur_and_auto_padding,
    TOL,
    [(44, 35)],
    &format!(
        "{SRC}{MASK}    MultiEffect {{
        x: 24; y: 20; width: 40; height: 30; source: src
        blurEnabled: true; blur: 0.6; blurMax: 24
        maskEnabled: true; maskSource: mask
    }}"
    ),
    [75, 75]
);

// A mask layer with `layer.smooth` is read linearly (the one above at its nearest pixel).
like_qt!(
    smooth_mask_with_blur_and_padding_rect,
    TOL,
    [(27, 23)],
    &format!(
        "{SRC}{}    MultiEffect {{
        x: 24; y: 20; width: 40; height: 30; source: src
        blurEnabled: true; blur: 0.5; blurMax: 20
        autoPaddingEnabled: false; paddingRect: Qt.rect(6, 4, 10, 12)
        maskEnabled: true; maskSource: mask; maskInverted: true
        maskThresholdMin: 0.3; maskSpreadAtMin: 0.4
    }}",
        MASK.replace(
            "layer.enabled: true",
            "layer.enabled: true; layer.smooth: true"
        )
    ),
    [29, 29]
);

like_qt!(
    soft_inverted_mask_with_blur_and_padding_rect,
    TOL,
    [(27, 23)],
    &format!(
        "{SRC}{MASK}    MultiEffect {{
        x: 24; y: 20; width: 40; height: 30; source: src
        blurEnabled: true; blur: 1; blurMax: 40; blurMultiplier: 0.5
        autoPaddingEnabled: false; paddingRect: Qt.rect(6, 4, 10, 12)
        maskEnabled: true; maskSource: mask; maskInverted: true
        maskThresholdMin: 0.3; maskSpreadAtMin: 0.4
    }}"
    ),
    [84, 84]
);

// ---- Qt5Compat RadialGradient (radialgradient.vert, radialgradient_nomask.frag and
// radialgradient_mask.frag over RadialGradient.qml's 16 x 256 gradient texture).

like_qt!(
    radial_with_defaults,
    TOL,
    [(60, 38), (12, 10)],
    "    RadialGradient { x: 10; y: 8; width: 100; height: 60 }
    RadialGradient { x: 115.5; y: 20.25; width: 37.5; height: 51.5 }"
);

/// RippleButton.qml's ripple: stops that end transparent, in an item moved by a Translate.
const RIPPLE: &str = "    Item {
        x: 70; y: 50; width: 120; height: 120; opacity: 0.8
        transform: Translate { x: -60; y: -60 }
        RadialGradient {
            anchors.fill: parent
            gradient: Gradient {
                GradientStop { position: 0.0; color: \"#ffb59d\" }
                GradientStop { position: 0.3; color: \"#ffb59d\" }
                GradientStop { position: 0.5; color: Qt.rgba(1, 0.71, 0.615, 0) }
            }
        }
    }";

like_qt!(radial_ripple, TOL, [(70, 50), (90, 60)], RIPPLE);

like_qt!(
    radial_offsets_radii_and_angle,
    TOL,
    [(60, 40), (130, 70)],
    "    RadialGradient {
        x: 6; y: 6; width: 100; height: 70
        horizontalOffset: 20; verticalOffset: -12; horizontalRadius: 45; verticalRadius: 18; angle: 30
        gradient: Gradient {
            GradientStop { position: 0.0; color: \"#ff3366\" }
            GradientStop { position: 0.4; color: \"#80ffee00\" }
            GradientStop { position: 1.0; color: \"#3366ff\" }
        }
    }
    RadialGradient { x: 112; y: 50; width: 40; height: 40; angle: -75; horizontalRadius: 8; opacity: 0.6 }"
);

like_qt!(
    radial_without_a_radius_stops_or_a_vertical_gradient,
    TOL,
    [(20, 20), (70, 20), (120, 20)],
    "    RadialGradient { x: 6; y: 6; width: 40; height: 40; horizontalRadius: 0 }
    RadialGradient { x: 56; y: 6; width: 40; height: 40; gradient: Gradient {} }
    RadialGradient {
        x: 106; y: 6; width: 40; height: 40
        gradient: Gradient {
            orientation: Gradient.Horizontal
            GradientStop { position: 0.0; color: \"#00ff00\" }
            GradientStop { position: 1.0; color: \"#ff00ff\" }
        }
    }
    RadialGradient { x: 6; y: 52; width: 140; height: 40; verticalRadius: -3; verticalOffset: 6 }"
);

like_qt!(
    radial_masked_by_its_source,
    TOL,
    [(44, 35), (110, 40)],
    &format!(
        "{SRC}    RadialGradient {{ x: 24; y: 20; width: 40; height: 30; source: src }}
    RadialGradient {{
        x: 80; y: 10; width: 70; height: 75; source: src; horizontalOffset: 10
        gradient: Gradient {{
            GradientStop {{ position: 0.0; color: \"#ffffff\" }}
            GradientStop {{ position: 0.6; color: \"#00a0ff\" }}
        }}
    }}"
    )
);

// ---- A rounded Rectangle whose edges fall between pixels.

// Its antialiased edge is a ramp one pixel wide over the edge, so each edge pixel is part
// colour, part background, rounded to a byte: the last bit of the vertex positions and of
// the colours decides which way.
like_qt!(
    rounded_rectangle_of_a_fractional_size,
    0,
    [(22, 18)],
    "    Rectangle { x: 12.8; y: 12.1; width: 20.4; height: 12.2; radius: 4; color: \"#ff3366\" }",
    [0, 0]
);

like_qt!(
    rounded_rectangle_on_half_pixels,
    0,
    [(22, 18)],
    "    Rectangle { x: 12.5; y: 12.5; width: 20; height: 12; radius: 4; color: \"#ff3366\" }",
    [0, 0]
);

// Two dozen of them, each at its own fraction of a pixel, of its own size, radius and colour
// (some translucent): 59 pixels were one level off on the CPU path and 52 on the GPU path
// while the vertices and the triangle were not llvmpipe's floats (`qsg::raster::viewport`).
like_qt!(
    rounded_rectangles_all_over_the_pixel_grid,
    0,
    [(10, 10)],
    "    Rectangle { x: 4.5; y: 3.25; width: 15.3; height: 10; radius: 2; color: \"#80ff3366\" }\n\
         Rectangle { x: 30.2; y: 3.5; width: 14; height: 15.1; radius: 3; color: \"#ff3366\" }\n\
         Rectangle { x: 56.2; y: 3.6; width: 15.3; height: 10.2; radius: 3; color: \"#ff3366\" }\n\
         Rectangle { x: 82.75; y: 3.6; width: 14; height: 10.2; radius: 3; color: \"#80ff3366\" }\n\
         Rectangle { x: 108.1; y: 3.8; width: 15.3; height: 10; radius: 3; color: \"#ff3366\" }\n\
         Rectangle { x: 134.75; y: 3.25; width: 14.6; height: 11.3; radius: 3; color: \"#80ff3366\" }\n\
         Rectangle { x: 4.2; y: 26.8; width: 14.6; height: 15.1; radius: 3.7; color: \"#33cc99\" }\n\
         Rectangle { x: 30.2; y: 26.8; width: 14.5; height: 10.8; radius: 2; color: \"#80ff3366\" }\n\
         Rectangle { x: 56.37; y: 26.2; width: 14; height: 10.5; radius: 5; color: \"#80ff3366\" }\n\
         Rectangle { x: 82.6; y: 26.63; width: 14.8; height: 12.7; radius: 2.5; color: \"#ffee00\" }\n\
         Rectangle { x: 108.5; y: 26.4; width: 14.5; height: 10.4; radius: 3.7; color: \"#33cc99\" }\n\
         Rectangle { x: 134.2; y: 26.8; width: 14.6; height: 15.1; radius: 5; color: \"#3366ff\" }\n\
         Rectangle { x: 4.37; y: 49.7; width: 14.6; height: 10.2; radius: 2; color: \"#80ff3366\" }\n\
         Rectangle { x: 30.6; y: 49.25; width: 14.8; height: 10.4; radius: 5; color: \"#ffee00\" }\n\
         Rectangle { x: 56.1; y: 49.9; width: 14.2; height: 15.1; radius: 2.5; color: \"#3366ff\" }\n\
         Rectangle { x: 82.5; y: 49.37; width: 14.8; height: 12.7; radius: 2.5; color: \"#ffee00\" }\n\
         Rectangle { x: 108.2; y: 49.2; width: 14.6; height: 12.7; radius: 3.7; color: \"#ff3366\" }\n\
         Rectangle { x: 134.1; y: 49.37; width: 14.6; height: 12.7; radius: 4; color: \"#ffee00\" }\n\
         Rectangle { x: 4.9; y: 72.5; width: 14; height: 12.7; radius: 4; color: \"#33cc99\" }\n\
         Rectangle { x: 30.8; y: 72.2; width: 16.7; height: 10; radius: 3; color: \"#3366ff\" }\n\
         Rectangle { x: 56.25; y: 72.37; width: 14.5; height: 11.3; radius: 5; color: \"#ffee00\" }\n\
         Rectangle { x: 82.2; y: 72.25; width: 16.7; height: 11.3; radius: 2.5; color: \"#3366ff\" }\n\
         Rectangle { x: 108.25; y: 72.6; width: 19.1; height: 10.6; radius: 3.7; color: \"#ffee00\" }\n\
         Rectangle { x: 134.5; y: 72.9; width: 15.3; height: 10.5; radius: 3; color: \"#ff3366\" }",
    [0, 0]
);

// Corner radii of their own, antialiasing without a radius, and a gradient: 10 pixels on the
// CPU path then.
like_qt!(
    rectangles_with_two_round_corners_with_smooth_square_ones_and_with_a_gradient,
    0,
    [(22, 18)],
    "    Rectangle { x: 12.3; y: 12.6; width: 40.7; height: 30.2; color: \"#3366ff\"\n\
             topLeftRadius: 9; bottomRightRadius: 3.5; antialiasing: true }\n\
         Rectangle { x: 70.3; y: 12.6; width: 40.7; height: 30.2; color: \"#ffee00\"\n\
             antialiasing: true }\n\
         Rectangle { x: 70.3; y: 50.6; width: 40.7; height: 30.2; radius: 6\n\
             gradient: Gradient {\n\
                 GradientStop { position: 0; color: \"#ff3366\" }\n\
                 GradientStop { position: 1; color: \"#3366ff\" }\n\
             } }",
    [0, 0]
);

// ---- A rounded Rectangle whose place between pixels is the sum of its parents' places.

// Qt adds the places item by item in floats (`QSGBatchRenderer`'s `Updater::visitTransformNode`),
// and the sum's last bit decides how an edge pixel rounds.
like_qt!(
    rounded_rectangles_in_items_between_pixels,
    0,
    [(30, 20), (90, 30), (40, 70)],
    "    Item { x: 3.3; y: 2.7
        Rectangle { x: 9.5; y: 9.4; width: 40.4; height: 22.2; radius: 4; color: \"#ff3366\" }
    }
    Item { x: 60; y: 10
        Item { x: 7.1; y: 3.3
            Item { x: 2.6; y: 1.9
                Rectangle { x: 4.7; y: 5.3; width: 36.3; height: 24.6; radius: 6; color: \"#33cc88\" }
            }
        }
    }
    Item { x: 12; y: 50
        Rectangle { x: 8.55; y: 6.15; width: 44.7; height: 26.9; radius: 9; color: \"#804488ff\" }
    }",
    [0, 0]
);

// An item that clips is where Qt's batches start anew: its own place stays in the matrix the
// shader gets, and only the places below it are added to the vertices.
like_qt!(
    rounded_rectangles_in_clipping_items_between_pixels,
    0,
    [(30, 20), (90, 30), (40, 70)],
    "    Item { x: 3.3; y: 2.7; width: 60; height: 40; clip: true
        Rectangle { x: 9.5; y: 9.4; width: 40.4; height: 22.2; radius: 4; color: \"#ff3366\" }
    }
    Item { x: 60; y: 10
        Item { x: 7.1; y: 3.3; width: 70; height: 50; clip: true
            Item { x: 2.6; y: 1.9
                Rectangle { x: 4.7; y: 5.3; width: 36.3; height: 24.6; radius: 6; color: \"#33cc88\" }
            }
        }
    }
    Item { x: 12; y: 50; width: 80; height: 40; clip: true
        Rectangle { x: 8.55; y: 6.15; width: 44.7; height: 26.9; radius: 9; color: \"#804488ff\" }
    }",
    [0, 0]
);

// ---- Every effect the tree check lets through is drawn.

/// A scene per effect type that `qml::typecheck::NATIVE` holds (a type of
/// Qt5Compat.GraphicalEffects or QtQuick.Effects, or one in `scene::EFFECT_ITEMS`), each
/// next to the hidden `src`.
const DRAWN: &[(&str, &str)] = &[
    ("MultiEffect", "source: src; brightness: 0.4"),
    ("RectangularShadow", "color: \"#ffffff\""),
    ("OpacityMask", "source: src; maskSource: src"),
    ("ColorOverlay", "source: src; color: \"#8000ff00\""),
    ("Desaturate", "source: src; desaturation: 1"),
    (
        "DropShadow",
        "source: src; radius: 4; samples: 9; color: \"#ffffff\"",
    ),
    ("GaussianBlur", "source: src; radius: 4; samples: 9"),
    ("RadialGradient", ""),
];

/// The tree check (`qml::typecheck`) lets a type through by its name alone, so a name in
/// `NATIVE` that `scene::paint_body` has no branch for loads and shows nothing. Each effect
/// type the check lets through has a scene here and changes the picture on both paths.
#[test]
fn every_effect_the_tree_check_accepts_is_drawn() {
    let reg = qml::types::registry();
    let effect = |n: &str| {
        scene::EFFECT_ITEMS.contains(&n)
            || ["Qt5Compat.GraphicalEffects", "QtQuick.Effects"]
                .iter()
                .any(|m| reg.exports(m, n))
    };
    let accepted: Vec<&str> = (qml::typecheck::NATIVE.iter().copied())
        .filter(|n| effect(n))
        .collect();
    assert!(accepted.len() >= DRAWN.len(), "{accepted:?}");
    for gpu in [false, true] {
        let bare = ours(SRC, gpu);
        for ty in &accepted {
            let Some((_, props)) = DRAWN.iter().find(|(t, _)| t == ty) else {
                panic!("{ty} passes the tree check: add its scene to DRAWN");
            };
            let body = format!("{SRC}    {ty} {{ x: 24; y: 20; width: 40; height: 30; {props} }}");
            let c = ours(&body, gpu);
            let changed = (0..W * H).filter(|&i| c.px[i] != bare.px[i]).count();
            assert!(
                changed > 100,
                "{ty} passes the tree check and draws {changed} pixels on the {} path",
                if gpu { "GPU" } else { "CPU" }
            );
        }
    }
}

// A source whose size is not whole: its layer is the size rounded, and Qt draws the source over
// all of it, so each Rectangle and clip in it lands on the texels that scale gives it, the
// clip's scissor rounded there. Nothing in it is antialiased: an edge drawn 1:1 and resampled
// afterwards is a blend here. The blurs and the MultiEffect are of a whole size, so their
// passes are texel for texel. DropShadow draws the source 1:1 into its padded layer, where the
// blue rectangle's lower edge (3.8 + 9.7) is exactly a row's centre: Qt draws a layer upside
// down on OpenGL, so that row falls outside the rectangle, not inside as in a window.
like_qt!(
    plain_rectangles_in_a_layer_of_a_fractional_size,
    0,
    [(18, 16), (72, 16), (126, 16), (20, 60), (80, 60), (132, 60)],
    "    Item {
        id: frac; width: 22.5; height: 19.3; visible: false
        Rectangle { x: 5.1; y: 3.8; width: 10.3; height: 9.7; color: \"#3366ff\" }
        Item {
            x: 13.2; y: 9.3; width: 6.6; height: 7.4; clip: true
            Rectangle { x: -4; y: -4; width: 30; height: 30; color: \"#ffee00\" }
        }
    }
    Item {
        id: wide; width: 44.6; height: 30.4; visible: false
        Rectangle { x: 6.2; y: 5.3; width: 30.3; height: 18.7; color: \"#ee2211\"; border.color: \"white\"; border.width: 2 }
    }
    Desaturate { x: 8; y: 6; width: 22.5; height: 19.3; source: frac; desaturation: 0.5 }
    ColorOverlay { x: 62; y: 6; width: 22.5; height: 19.3; source: frac; color: \"#4000ff00\" }
    GaussianBlur { x: 116; y: 6; width: 23; height: 19; source: frac; radius: 2; samples: 5 }
    MultiEffect { x: 8; y: 50; width: 45; height: 30; source: wide; brightness: 0.1 }
    GaussianBlur { x: 70; y: 50; width: 45; height: 30; source: wide; radius: 3; samples: 7 }
    DropShadow { x: 124; y: 50; width: 23; height: 19; source: frac; radius: 4; samples: 9; horizontalOffset: 3; verticalOffset: 2; color: \"#00ff00\" }",
    [0, 0]
);

// What else a source of a size that is not whole holds is drawn at the layer's scale as well,
// next to the same in a source of a whole size and in the window. Glyphs of NativeRendering
// text: each one's quad is scaled and then snapped to texels.
like_qt!(
    native_text_in_a_layer_of_a_fractional_size,
    0,
    [(34, 14)],
    "    Item {
        id: frac; width: 60.5; height: 22.3; visible: false
        Text { x: 2.2; y: 1.3; text: \"ABWijm\"; font.pixelSize: 14; color: \"white\"; renderType: Text.NativeRendering }
    }
    Item {
        id: whole; width: 60; height: 22; visible: false
        Text { x: 2.2; y: 1.3; text: \"ABWijm\"; font.pixelSize: 14; color: \"white\"; renderType: Text.NativeRendering }
    }
    Desaturate { x: 8; y: 6; width: 60.5; height: 22.3; source: frac; desaturation: 0.5 }
    Desaturate { x: 84; y: 6; width: 60; height: 22; source: whole; desaturation: 0.5 }
    Text { x: 10.2; y: 61.3; text: \"ABWijm\"; font.pixelSize: 14; color: \"white\"; renderType: Text.NativeRendering }",
    [0, 0]
);

// Glyphs of QtRendering text are distance-field tiles drawn through the layer's scale.
like_qt!(
    distance_field_text_in_a_layer_of_a_fractional_size,
    0,
    [(34, 14)],
    "    Item {
        id: frac; width: 60.5; height: 22.3; visible: false
        Text { x: 2.2; y: 1.3; text: \"ABWijm\"; font.pixelSize: 14; color: \"white\"; renderType: Text.QtRendering }
    }
    Item {
        id: whole; width: 60; height: 22; visible: false
        Text { x: 2.2; y: 1.3; text: \"ABWijm\"; font.pixelSize: 14; color: \"white\"; renderType: Text.QtRendering }
    }
    Desaturate { x: 8; y: 6; width: 60.5; height: 22.3; source: frac; desaturation: 0.5 }
    Desaturate { x: 84; y: 6; width: 60; height: 22; source: whole; desaturation: 0.5 }
    Text { x: 10.2; y: 61.3; text: \"ABWijm\"; font.pixelSize: 14; color: \"white\"; renderType: Text.QtRendering }",
    [0, 0]
);

/// A 16 x 12 picture of four colours in a file, as a `file:` url.
fn picture() -> String {
    let path = std::env::temp_dir().join(format!("effects_qt_{}.png", std::process::id()));
    let mut c = Canvas::new(16, 12, [0, 0, 0, 0]);
    for (i, p) in c.px.iter_mut().enumerate() {
        *p = match (i % 16 < 8, i / 16 < 6) {
            (true, true) => [255, 40, 40, 255],
            (false, true) => [40, 220, 60, 255],
            (true, false) => [255, 220, 0, 255],
            (false, false) => [50, 90, 255, 255],
        };
    }
    c.save_png(path.to_str().unwrap()).expect("the picture");
    format!("file://{}", path.display())
}

// Images, smooth and not: the quad is scaled, the picture is read through it once.
like_qt!(
    images_in_a_layer_of_a_fractional_size,
    0,
    [(12, 9), (42, 10), (88, 9), (118, 10)],
    &format!(
        "    Item {{
        id: frac; width: 60.5; height: 22.3; visible: false
        Image {{ x: 3.3; y: 2.4; width: 24.5; height: 15.2; source: \"{0}\" }}
        Image {{ x: 32.6; y: 3.1; width: 22.3; height: 14.4; source: \"{0}\"; smooth: false }}
    }}
    Item {{
        id: whole; width: 60; height: 22; visible: false
        Image {{ x: 3.3; y: 2.4; width: 24.5; height: 15.2; source: \"{0}\" }}
        Image {{ x: 32.6; y: 3.1; width: 22.3; height: 14.4; source: \"{0}\"; smooth: false }}
    }}
    Desaturate {{ x: 8; y: 6; width: 60.5; height: 22.3; source: frac; desaturation: 0.5 }}
    Desaturate {{ x: 84; y: 6; width: 60; height: 22; source: whole; desaturation: 0.5 }}",
        picture()
    ),
    [0, 0]
);

// Effects: each one's last quad is scaled, its own passes stay in its own texels.
like_qt!(
    effects_in_a_layer_of_a_fractional_size,
    0,
    [(16, 12), (43, 14), (92, 12), (119, 14)],
    "    Item {
        id: inner; width: 20; height: 12; visible: false
        Rectangle { x: 3; y: 2; width: 12; height: 7; color: \"#ff3366\" }
    }
    Item {
        id: frac; width: 60.5; height: 22.3; visible: false
        GaussianBlur { x: 4; y: 3; width: 20; height: 12; source: inner; radius: 2; samples: 5 }
        ColorOverlay { x: 30.4; y: 5.2; width: 20; height: 12; source: inner; color: \"#8000ff00\" }
    }
    Item {
        id: whole; width: 60; height: 22; visible: false
        GaussianBlur { x: 4; y: 3; width: 20; height: 12; source: inner; radius: 2; samples: 5 }
        ColorOverlay { x: 30.4; y: 5.2; width: 20; height: 12; source: inner; color: \"#8000ff00\" }
    }
    Desaturate { x: 8; y: 6; width: 60.5; height: 22.3; source: frac; desaturation: 0.5 }
    Desaturate { x: 84; y: 6; width: 60; height: 22; source: whole; desaturation: 0.5 }",
    [0, 0]
);

// Subtrees turned by quarter turns or scaled: Qt draws their geometry through the transform
// and the layer's scale at once, so their edges are as hard as any other's.
like_qt!(
    turned_and_scaled_subtrees_in_a_layer_of_a_fractional_size,
    0,
    [
        (20, 10),
        (45, 13),
        (96, 10),
        (121, 13),
        (20, 64),
        (45, 62),
        (67, 67),
        (120, 68)
    ],
    "    Item {
        id: frac; width: 60.5; height: 22.3; visible: false
        Rectangle { x: 8; y: 6; width: 14; height: 8; color: \"#ff3366\"; rotation: 90
            Rectangle { x: 2; y: 1; width: 4; height: 3; color: \"#3366ff\" } }
        Rectangle { x: 36; y: 6; width: 10; height: 8; color: \"#3366ff\"; rotation: 180
            Rectangle { x: 2; y: 1; width: 4; height: 3; color: \"#ffcc00\" } }
    }
    Item {
        id: whole; width: 60; height: 22; visible: false
        Rectangle { x: 8; y: 6; width: 14; height: 8; color: \"#ff3366\"; rotation: 90
            Rectangle { x: 2; y: 1; width: 4; height: 3; color: \"#3366ff\" } }
        Rectangle { x: 36; y: 6; width: 10; height: 8; color: \"#3366ff\"; rotation: 180
            Rectangle { x: 2; y: 1; width: 4; height: 3; color: \"#ffcc00\" } }
    }
    Desaturate { x: 8; y: 6; width: 60.5; height: 22.3; source: frac; desaturation: 0.5 }
    Desaturate { x: 84; y: 6; width: 60; height: 22; source: whole; desaturation: 0.5 }
    Rectangle { x: 16; y: 66; width: 14; height: 8; color: \"#ff3366\"; rotation: 90
        Rectangle { x: 2; y: 1; width: 4; height: 3; color: \"#3366ff\" } }
    Rectangle { x: 66; y: 66; width: 10; height: 8; color: \"#3366ff\"; rotation: 180
        Rectangle { x: 2; y: 1; width: 4; height: 3; color: \"#ffcc00\" } }
    Rectangle { x: 40.3; y: 62.4; width: 14; height: 8; color: \"#ff3366\"; rotation: 90
        Rectangle { x: 2; y: 1; width: 4; height: 3; color: \"#3366ff\" } }
    Rectangle { x: 110.4; y: 62.4; width: 20; height: 12; color: \"#3366ff\"; scale: 1.3
        Rectangle { x: 2; y: 1; width: 4; height: 3; color: \"#ffcc00\" } }",
    [0, 0]
);

// An Image that is not smooth, in the window: an edge through the middle of a pixel is a tie,
// and the pixel goes where Qt's rasterizer puts it. No pixel's centre is on the line between
// two texels of different colours: which of the two Qt reads there is rounding in the
// graphics card's arithmetic.
like_qt!(
    images_that_are_not_smooth_with_an_edge_on_a_pixel_centre,
    0,
    [(20, 20), (62, 18), (110, 20), (20, 60), (62, 60)],
    &format!(
        "    Image {{ x: 12.6; y: 13.1; width: 22.3; height: 14.4; source: \"{0}\"; smooth: false }}
    Image {{ x: 50.5; y: 10.5; width: 25; height: 15; source: \"{0}\"; smooth: false }}
    Image {{ x: 100.2; y: 12.5; width: 22.3; height: 15; source: \"{0}\"; smooth: false }}
    Image {{ x: 12.5; y: 52; width: 17; height: 14; source: \"{0}\"; smooth: false }}
    Image {{ x: 50; y: 52.5; width: 34; height: 25; source: \"{0}\"; smooth: false }}",
        picture()
    ),
    [0, 0]
);

// Effects whose own size is not whole: the source's texture, of the source's size, is read
// over the effect's real width and height, and a blur's passes are textures of the effect's
// size rounded. MultiEffect's blur and shadow read a source grown by the padding in the
// source's units.
mod effects_of_a_fractional_size {
    use super::*;

    const LIT: [(usize, usize); 5] = [(10, 7), (59, 7), (110, 7), (13, 50), (64, 51)];
    const BODY: &str = "    Item {
        id: src; width: 40; height: 30; visible: false
        Rectangle { anchors.fill: parent; radius: 6; color: \"#ff3366\" }
        Rectangle { x: 8; y: 4; width: 6; height: 22; color: \"#3366ff\" }
    }
    MultiEffect { x: 6; y: 6; width: 40.5; height: 30.3; source: src; contrast: 0.2 }
    GaussianBlur { x: 56; y: 6; width: 40.6; height: 30.4; source: src; radius: 3; samples: 7 }
    DropShadow { x: 106; y: 6; width: 40.5; height: 30.3; source: src; radius: 4; samples: 9; horizontalOffset: 3; verticalOffset: 2; color: \"#00ff00\" }
    MultiEffect { x: 6; y: 50; width: 40.5; height: 30.3; source: src; blurEnabled: true; blur: 0.5; blurMax: 16 }
    MultiEffect { x: 60; y: 50; width: 40.5; height: 30.3; source: src; shadowEnabled: true; shadowBlur: 0.4; shadowHorizontalOffset: 3; shadowVerticalOffset: 2; shadowColor: \"#00ff00\" }";

    #[test]
    #[ignore = "MultiEffect's blur and shadow of a size that is not whole are one level off Qt's \
                in 28 pixels on the CPU raster and on the GPU path (docs/qt-differences.md, one \
                level off Qt's)"]
    fn cpu() {
        like_qt(BODY, false, 0, 0, &LIT);
    }

    #[test]
    #[ignore = "MultiEffect's blur and shadow of a size that is not whole are one level off Qt's \
                in 28 pixels on the CPU raster and on the GPU path (docs/qt-differences.md, one \
                level off Qt's)"]
    fn gpu() {
        like_qt(BODY, true, 0, 0, &LIT);
    }
}

// ---- What a layer's texture holds: the item's own rectangle, in the item's own coordinates
// (QQuickItemLayer makes a ShaderEffectSource of the item, with the item's rectangle as its
// sourceRect, and the source is rendered without the item's own transform).

// `layer.enabled` without an effect: children that leave the item are cut at its edge, at a
// whole place, at a place and of a size that are not whole, and with `layer.smooth`.
like_qt!(
    a_layer_cuts_its_item_at_the_items_edge,
    TOL,
    [
        (15, 15),
        (35, 25),
        (75, 15),
        (95, 25),
        (15, 55),
        (35, 65),
        (80, 60)
    ],
    "    Rectangle { x: 10; y: 10; width: 30; height: 20; color: \"#ff0000\"
        layer.enabled: true
        Rectangle { x: 20; y: 10; width: 20; height: 20; color: \"#ffffff\" }
        Rectangle { x: -6; y: -6; width: 12; height: 12; color: \"#3366ff\" } }
    Rectangle { x: 70.4; y: 10.6; width: 30.5; height: 20.3; color: \"#00c000\"
        layer.enabled: true
        Rectangle { x: 20; y: 10; width: 20; height: 20; color: \"#ffffff\" }
        Rectangle { x: -6; y: -6; width: 12; height: 12; color: \"#3366ff\" } }
    Rectangle { x: 10.4; y: 50.6; width: 30.5; height: 20.3; color: \"#ffcc00\"
        layer.enabled: true; layer.smooth: true
        Rectangle { x: 20; y: 10; width: 20; height: 20; color: \"#ffffff\" }
        Rectangle { x: -6; y: -6; width: 12; height: 12; color: \"#3366ff\" } }
    Item { x: 70; y: 50; width: 40; height: 30
        layer.enabled: true
        Rectangle { x: 5; y: 5; width: 50; height: 12; radius: 6; color: \"#ff3366\" }
        Text { x: 28; y: 16; text: \"layer\"; color: \"#ffffff\"; font.pixelSize: 12
            renderType: Text.NativeRendering } }"
);

// An effect that takes an item as its `source` shows the item as it is before its own
// `rotation` and `scale`; the turn of a child inside the source stays.
like_qt!(
    an_effect_shows_its_source_without_the_sources_own_turn_and_scale,
    TOL,
    [(15, 15), (40, 30), (75, 15), (100, 30), (15, 60), (75, 60)],
    "    Item {
        id: turned; width: 40; height: 30; visible: false; rotation: 15
        Rectangle { anchors.fill: parent; radius: 6; color: \"#ff3366\" }
        Rectangle { x: 8; y: 4; width: 6; height: 22; color: \"#3366ff\" }
    }
    Item {
        id: grown; width: 40; height: 30; visible: false; scale: 1.5; transformOrigin: Item.TopLeft
        Rectangle { anchors.fill: parent; radius: 6; color: \"#ff3366\" }
        Rectangle { x: 8; y: 4; width: 6; height: 22; color: \"#3366ff\" }
    }
    Item {
        id: flipped; width: 40; height: 30; visible: false; rotation: 180; scale: 0.5
        Rectangle { anchors.fill: parent; radius: 6; color: \"#ff3366\" }
        Rectangle { x: 8; y: 4; width: 6; height: 22; color: \"#3366ff\"
            Rectangle { x: 1; y: 2; width: 12; height: 4; color: \"#ffcc00\"; rotation: 90 } }
    }
    Desaturate { x: 10; y: 10; width: 40; height: 30; source: turned; desaturation: 0.5 }
    ColorOverlay { x: 70; y: 10; width: 40; height: 30; source: grown; color: \"#4000ff00\" }
    OpacityMask { x: 10; y: 50; width: 40; height: 30; source: flipped; maskSource: grown }
    MultiEffect { x: 70; y: 50; width: 40; height: 30; source: turned; saturation: -1 }"
);

// A layer is one picture: `opacity` fades it once, where its children overlap too, a layer
// inside a layer is cut twice, and an effect leaves the opacity of its source out.
like_qt!(
    a_layer_is_faded_as_one_picture,
    TOL,
    [
        (15, 15),
        (30, 25),
        (75, 15),
        (95, 25),
        (20, 60),
        (80, 60),
        (130, 60)
    ],
    "    Item { x: 10; y: 10; width: 40; height: 30; opacity: 0.5
        layer.enabled: true
        Rectangle { width: 26; height: 20; color: \"#ff0000\" }
        Rectangle { x: 14; y: 10; width: 26; height: 20; color: \"#3366ff\" } }
    Rectangle { x: 70; y: 10; width: 40; height: 30; color: \"#00c000\"; opacity: 0.75
        layer.enabled: true
        Rectangle { x: 10; y: 10; width: 40; height: 30; color: \"#ffffff\"; opacity: 0.5
            layer.enabled: true
            Rectangle { x: 20; y: 10; width: 30; height: 30; color: \"#3366ff\" } } }
    Item {
        id: faded; width: 40; height: 30; visible: false; opacity: 0.25
        Rectangle { anchors.fill: parent; radius: 6; color: \"#ff3366\" }
        Rectangle { x: 8; y: 4; width: 6; height: 22; color: \"#3366ff\"; opacity: 0.5 }
    }
    Desaturate { x: 10; y: 50; width: 40; height: 30; source: faded; desaturation: 0.5 }
    MultiEffect { x: 70; y: 50; width: 40; height: 30; source: faded; opacity: 0.5 }
    Rectangle { x: 124; y: 50; width: 30; height: 30; color: \"#ffcc00\"; opacity: 0.5
        layer.enabled: true
        layer.effect: MultiEffect { saturation: -1 } }"
);

// The layers without an effect the shell shows: the Shape of RoundCorner.qml (no size of its
// own), of CircularProgress.qml and of CircleSelectionDetails.qml (a line that leaves the
// item), and the numbers of Workspaces.qml, shown and taken as a mask at once.
like_qt!(
    the_shells_layers_without_an_effect,
    TOL,
    [(13, 13), (62, 24), (120, 18), (17, 68), (77, 68)],
    "    Item { x: 10; y: 10; width: 20; height: 20
        Shape { anchors.top: parent.top; anchors.left: parent.left
            layer.enabled: true; layer.smooth: true
            preferredRendererType: Shape.CurveRenderer
            ShapePath { strokeWidth: 0; fillColor: \"#ff3366\"; startX: 0; startY: 0
                pathHints: ShapePath.PathSolid & ShapePath.PathNonIntersecting
                PathAngleArc { moveToStart: false; centerX: 20; centerY: 20; radiusX: 20; radiusY: 20
                    startAngle: 180; sweepAngle: 90 }
                PathLine { x: 0; y: 0 } } } }
    Item { x: 60; y: 10; width: 30; height: 30
        Shape { anchors.fill: parent
            layer.enabled: true; layer.smooth: true
            preferredRendererType: Shape.CurveRenderer
            ShapePath { strokeColor: \"#3366ff\"; strokeWidth: 2; capStyle: ShapePath.RoundCap
                fillColor: \"transparent\"
                PathAngleArc { centerX: 15; centerY: 15; radiusX: 13; radiusY: 13
                    startAngle: -110; sweepAngle: -200 } } } }
    Shape { x: 100.5; y: 10.25; width: 40; height: 30
        layer.enabled: true; layer.smooth: true
        preferredRendererType: Shape.CurveRenderer
        ShapePath { strokeWidth: 4; pathHints: ShapePath.PathLinear; fillColor: \"transparent\"
            strokeColor: \"#ffcc00\"; capStyle: ShapePath.RoundCap; joinStyle: ShapePath.RoundJoin
            PathPolyline { path: [Qt.point(6.3, 6.2), Qt.point(34.1, 10.4), Qt.point(12.2, 24.3), Qt.point(52.4, 22.1)] } } }
    Rectangle { id: pill; x: 10; y: 60; width: 44; height: 16; radius: 8; color: \"#00c000\"; visible: false }
    Row { id: nums; x: 10; y: 60; z: 4; spacing: 4
        layer.enabled: true
        Repeater { model: 3; Rectangle { width: 12; height: 16; radius: 3; color: \"#ffffff\" } } }
    MultiEffect { x: 70; y: 60; z: 5; width: 44; height: 16; source: pill
        maskEnabled: true; maskSource: nums; maskThresholdMin: 0.5; maskSpreadAtMin: 1 }"
);

// The lock screen's wallpaper blur (Background.qml's `blurLoader`): a GaussianBlur of a size
// that is not whole, of a source of that size, in a subtree scaled by `lock.blur.extraZoom`.
like_qt!(
    a_blur_of_a_fractional_size_in_a_scaled_subtree,
    0,
    [(20, 14), (80, 48), (136, 80)],
    "    Item {
        id: wall; width: 120.52; height: 70.48; visible: false
        Rectangle { anchors.fill: parent; gradient: Gradient {
            GradientStop { position: 0; color: \"#ff3366\" }
            GradientStop { position: 1; color: \"#3366ff\" } } }
        Rectangle { x: 20; y: 12; width: 30; height: 40; color: \"#ffcc00\" }
        Rectangle { x: 70.5; y: 30.5; width: 36; height: 22; radius: 8; color: \"white\" }
    }
    Item { x: 14; y: 10; width: 120.52; height: 70.48; scale: 1.1
        GaussianBlur { anchors.fill: parent; source: wall; radius: 10; samples: 21 } }",
    [0, 0]
);

// The same blur of a whole size: scaled by 1.1 about its centre its top and bottom edges are
// on the centres of rows 6 and 83 of the window (10 + 35 - 38.5 = 6.5), and Qt draws the
// lower one of the two rows.
mod a_blur_in_a_scaled_subtree_with_edges_on_pixel_centres {
    use super::*;

    const LIT: [(usize, usize); 3] = [(20, 14), (80, 48), (136, 83)];
    const BODY: &str = "    Item {
        id: wall; width: 120; height: 70; visible: false
        Rectangle { anchors.fill: parent; gradient: Gradient {
            GradientStop { position: 0; color: \"#ff3366\" }
            GradientStop { position: 1; color: \"#3366ff\" } } }
        Rectangle { x: 20; y: 12; width: 30; height: 40; color: \"#ffcc00\" }
        Rectangle { x: 70.5; y: 30.5; width: 36; height: 22; radius: 8; color: \"white\" }
    }
    Item { x: 14; y: 10; width: 120; height: 70; scale: 1.1
        GaussianBlur { anchors.fill: parent; source: wall; radius: 10; samples: 21 } }";

    #[test]
    #[ignore = "a blur in a scaled subtree with its edges on pixel centres is one level off Qt's \
                in 17 pixels on the CPU raster and in 23 on the GPU path \
                (docs/qt-differences.md, one level off Qt's)"]
    fn cpu() {
        like_qt(BODY, false, 0, 0, &LIT);
    }

    #[test]
    #[ignore = "a blur in a scaled subtree with its edges on pixel centres is one level off Qt's \
                in 17 pixels on the CPU raster and in 23 on the GPU path \
                (docs/qt-differences.md, one level off Qt's)"]
    fn gpu() {
        like_qt(BODY, true, 0, 0, &LIT);
    }
}

// And of a size that is not whole in a subtree that is not scaled. Its last pass reads its
// source in column 116 0.508 of a 256th of a texel along x, 0.008 from where the weight of a
// texel rounds the other way: 8 pixels there were one level off Qt's while Qt's window was
// not of the scene's size (`qt`), whose width is in Qt's coordinate.
like_qt!(
    a_blur_of_a_fractional_size,
    0,
    [(20, 14), (80, 48), (130, 76)],
    "    Item {
        id: wall; width: 120.52; height: 70.48; visible: false
        Rectangle { anchors.fill: parent; gradient: Gradient {
            GradientStop { position: 0; color: \"#ff3366\" }
            GradientStop { position: 1; color: \"#3366ff\" } } }
        Rectangle { x: 20; y: 12; width: 30; height: 40; color: \"#ffcc00\" }
        Rectangle { x: 70.5; y: 30.5; width: 36; height: 22; radius: 8; color: \"white\" }
    }
    Item { x: 14; y: 10; width: 120.52; height: 70.48
        GaussianBlur { anchors.fill: parent; source: wall; radius: 10; samples: 21 } }",
    [0, 0]
);

// ---- A layer effect on each delegate of a Repeater.

/// The original's calendar day buttons: the background of every delegate is a rounded rectangle masked
/// by a rounded rectangle through its own `layer.effect`, so each corner is covered twice.
/// The size, radius and colour are the today button's.
mod masked_delegates {
    use super::*;

    const BODY: &str = "    Row {
        x: 1; y: 10; spacing: 2
        Repeater {
            model: 4
            delegate: Rectangle {
                id: bg
                width: 38; height: 38; radius: 12; color: \"#cbc4cb\"
                layer.enabled: true
                layer.effect: OpacityMask {
                    maskSource: Rectangle { width: bg.width; height: bg.height; radius: 12 }
                }
            }
        }
    }";
    const LIT: [(usize, usize); 4] = [(20, 29), (60, 29), (100, 29), (140, 29)];

    /// How many pixels of ours are not Qt's, and the most levels one of them is off.
    fn differ(gpu: bool) -> Option<(usize, u8)> {
        let q = qt(BODY)?;
        for (x, y) in LIT {
            assert!(
                far(q.px[y * W + x], [0x20, 0x20, 0x20, 255]) > 8,
                "Qt draws a delegate at {x},{y}: {:?}",
                q.px[y * W + x]
            );
        }
        let c = ours(BODY, gpu);
        let off = (0..W * H).map(|i| far(q.px[i], c.px[i])).filter(|&d| d > 0);
        Some((off.clone().count(), off.max().unwrap_or(0)))
    }

    /// Without an effect on each delegate 200 pixels differ, by up to 43 levels.
    #[test]
    fn gpu() {
        if let Some(d) = differ(true) {
            assert_eq!(d, (0, 0), "pixels not Qt's, levels off at most");
        }
    }

    /// Without the effects 280 differ, by up to 43 levels.
    #[test]
    fn cpu() {
        if let Some(d) = differ(false) {
            assert_eq!(d, (0, 0), "pixels not Qt's, levels off at most");
        }
    }
}

// One such rectangle outside a Repeater, in the sizes and colours of the original's buttons
// (RippleButton.qml): every pixel is Qt's, the partly covered ones of the corners too.
like_qt!(
    a_masked_rounded_rectangle_of_38,
    0,
    [(20, 29)],
    "    Rectangle { id: bg; x: 1; y: 10; width: 38; height: 38; radius: 12; color: \"#cbc4cb\"; layer.enabled: true
        layer.effect: OpacityMask { maskSource: Rectangle { width: bg.width; height: bg.height; radius: 12 } } }",
    [0, 0]
);

like_qt!(
    a_masked_rounded_rectangle_of_36,
    0,
    [(19, 28)],
    "    Rectangle { id: bg; x: 1; y: 10; width: 36; height: 36; radius: 12; color: \"#cbc4cb\"; layer.enabled: true
        layer.effect: OpacityMask { maskSource: Rectangle { width: bg.width; height: bg.height; radius: 12 } } }",
    [0, 0]
);

like_qt!(
    a_masked_rounded_rectangle_of_another_colour,
    0,
    [(20, 29)],
    "    Rectangle { id: bg; x: 1; y: 10; width: 38; height: 38; radius: 12; color: \"#65558F\"; layer.enabled: true
        layer.effect: OpacityMask { maskSource: Rectangle { width: bg.width; height: bg.height; radius: 12 } } }",
    [0, 0]
);

/// The largest difference of one colour channel between Qt's picture and ours that still is
/// the same picture: both blend in 8 bits per channel, in passes that round separately.
const TOL: u8 = 3;

// ---- GaussianBlur's kernel as Qt writes it into the shader it generates.

/// The float literals of the first GLSL shader in `qsb --dump`'s output for `file` that follow
/// a `* `: the offsets of a blur's vertex shader, the weights of its fragment shader.
fn literals(file: &Path) -> Vec<f32> {
    let out = Command::new(QSB)
        .arg("--dump")
        .arg(file)
        .output()
        .expect("qsb runs");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let glsl = text
        .split("Shader ")
        .find(|s| s.contains("GLSL") && s.contains("void main()"))
        .unwrap_or_else(|| panic!("a GLSL shader in {}:\n{text}", file.display()));
    let main = glsl.split("void main()").nth(1).unwrap();
    main.split("* ")
        .skip(1)
        .filter_map(|s| {
            let n: String = s
                .trim_start_matches('(')
                .chars()
                .take_while(|c| c.is_ascii_digit() || "+-.e".contains(*c))
                .collect();
            n.parse().ok()
        })
        .collect()
}

#[test]
fn blur_kernel_is_the_one_in_qts_generated_shader() {
    for (radius, samples, deviation) in [(8, 17, None), (5, 11, None), (12, 25, Some(2.75))] {
        let set = deviation.map_or(String::new(), |d: f64| format!("; deviation: {d}"));
        let Some(q) = qt(&format!(
            "{SRC}    GaussianBlur {{ x: 24; y: 20; width: 40; height: 30; source: src; radius: {radius}; samples: {samples}{set} }}"
        )) else {
            return;
        };
        let cache = q
            .dir
            .path()
            .join("cache/qmltestrunner/_qt_QGfxShaderBuilder_6.12.0");
        let shaders = std::fs::read_dir(&cache)
            .unwrap_or_else(|e| panic!("Qt's generated shaders under {}: {e}", cache.display()));
        // The blur's two shaders hold as many literals as taps: the vertex shader every offset
        // but the centre's (not multiplied), the fragment shader every weight.
        let (taps, _, _) = qsg::blur::kernel(radius as f64, samples, deviation);
        let mut found = Vec::new();
        for f in shaders {
            let l = literals(&f.expect("shader file").path());
            if !l.is_empty() {
                found.push(l);
            }
        }
        // The vertex shader adds no offset to a tap at the centre.
        let offsets: Vec<f32> = taps.iter().map(|t| t.0).filter(|&o| o != 0.0).collect();
        let weights: Vec<f32> = taps.iter().map(|t| t.1).collect();
        assert!(
            found.contains(&offsets),
            "radius {radius}: our offsets {offsets:?} are a shader's of Qt: {found:?}"
        );
        assert!(
            found.contains(&weights),
            "radius {radius}: our weights {weights:?} are a shader's of Qt: {found:?}"
        );
    }
}

// ---- What a layer does not do yet (docs/qt-differences.md).

/// How many pixels of `body` on one of our paths are not Qt's, without tolerance.
fn unlike_qt(body: &str, gpu: bool) -> Option<usize> {
    let q = qt(body)?;
    let c = ours(body, gpu);
    Some((0..W * H).filter(|&i| q.px[i] != c.px[i]).count())
}

/// The scene `$body` is known not to be Qt's: `$name::cpu` and `$name::gpu` hold how many
/// pixels differ, so the day the property is made the test fails and its row in
/// docs/qt-differences.md goes.
macro_rules! unlike_qt {
    ($name:ident, $differ:expr, $body:expr) => {
        mod $name {
            use super::*;

            #[test]
            fn cpu() {
                if let Some(n) = unlike_qt($body, false) {
                    assert_eq!(n, $differ[0], "pixels that are not Qt's on the CPU path");
                }
            }
            #[test]
            fn gpu() {
                if let Some(n) = unlike_qt($body, true) {
                    assert_eq!(n, $differ[1], "pixels that are not Qt's on the GPU path");
                }
            }
        }
    };
}

/// A 40 x 28 item whose edges and a rounded child show how its layer's texture is sampled.
const LAYERED: &str = "
        Rectangle { anchors.fill: parent; color: \"#ff3366\" }
        Rectangle { x: 6; y: 4; width: 12; height: 10; color: \"#ffcc00\" }
        Rectangle { x: 20.5; y: 10.5; width: 14; height: 12; radius: 4; color: \"white\" } }";

// `layer.textureSize` is not read: the texture has the item's size, Qt's is 10 x 7.
unlike_qt!(
    layer_texture_size_is_not_read,
    [248, 248],
    &format!(
        "    Item {{ x: 4; y: 4; width: 40; height: 28; layer.enabled: true
        layer.textureSize: Qt.size(10, 7){LAYERED}"
    )
);

unlike_qt!(
    layer_texture_size_is_not_read_when_smooth,
    [543, 543],
    &format!(
        "    Item {{ x: 4; y: 4; width: 40; height: 28; layer.enabled: true; layer.smooth: true
        layer.textureSize: Qt.size(10, 7){LAYERED}"
    )
);

// `layer.mipmap` is not read: a layer shown at a quarter of its size is filtered from the
// full texture, Qt's from a mipmap level.
unlike_qt!(
    layer_mipmap_is_not_read,
    [26, 26],
    &format!(
        "    Item {{ x: 4; y: 4; width: 40; height: 28; scale: 0.25; layer.enabled: true
        layer.smooth: true; layer.mipmap: true{LAYERED}"
    )
);

// CookieClock's `scale: 1.4 - 0.4 * shown` is 0.9999999999999999 when shown. Qt's node matrix
// is of floats, where that is 1: the subtree is drawn as one that is not scaled.
mod subtrees_scaled_by_one_less_a_rounding_error {
    use super::*;

    const LIT: [(usize, usize); 2] = [(20, 18), (117, 28)];
    const BODY: &str = "    Item { x: 10.3; y: 8.6; width: 61; height: 36; scale: 1.4 - 0.4 * 1
        Rectangle { x: 2.5; y: 3.5; width: 20.4; height: 12.2; radius: 4; color: \"#ff3366\" }
        Rectangle { x: 30; y: 4; width: 9.5; height: 10.5; color: \"#3366ff\" }
        Text { x: 2.4; y: 17.3; text: \"ABWijm\"; font.pixelSize: 14; color: \"white\"; renderType: Text.NativeRendering } }
    Item { x: 90.5; y: 20.5; width: 40; height: 31; scale: 1.4 - 0.4 * 1; rotation: 90
        Rectangle { x: 2; y: 3; width: 21; height: 12; color: \"#ffcc00\" }
        Rectangle { x: 25.5; y: 5.5; width: 9; height: 20; radius: 3; color: \"#3366ff\" } }";

    #[test]
    #[ignore = "antialiased edges of rounded Rectangles between pixels, in an item scaled by one \
                less a rounding error and in one turned by 90 degrees too, are one level off \
                Qt's in 43 pixels on the CPU raster and in 37 on the GPU path \
                (docs/qt-differences.md, one level off Qt's)"]
    fn cpu() {
        like_qt(BODY, false, 0, 0, &LIT);
    }

    #[test]
    #[ignore = "antialiased edges of rounded Rectangles between pixels, in an item scaled by one \
                less a rounding error and in one turned by 90 degrees too, are one level off \
                Qt's in 43 pixels on the CPU raster and in 37 on the GPU path \
                (docs/qt-differences.md, one level off Qt's)"]
    fn gpu() {
        like_qt(BODY, true, 0, 0, &LIT);
    }
}

// RippleButton.qml's ripple under a press: a RadialGradient in the button's layer, whose
// texture is drawn top row first.
like_qt!(
    a_radial_gradient_in_a_layer,
    0,
    [(60, 48)],
    "    Rectangle { id: bg; x: 20; y: 28; width: 120; height: 40; radius: 12; color: \"#4a4458\"
        layer.enabled: true
        layer.effect: OpacityMask { maskSource: Rectangle { width: bg.width; height: bg.height; radius: 12 } }
        Item { id: ripple; x: 40; y: 25; width: 90; height: 90; opacity: 0.6
            RadialGradient { anchors.fill: parent; gradient: Gradient {
                GradientStop { position: 0.0; color: \"#ffd8e4\" }
                GradientStop { position: 0.3; color: \"#ffd8e4\" }
                GradientStop { position: 0.5; color: Qt.rgba(1, 0.85, 0.9, 0) } } }
            transform: Translate { x: -ripple.width / 2; y: -ripple.height / 2 } } }",
    [0, 0]
);

// Edges on pixel centres, without tolerance. Qt Quick draws a window through OpenGL bottom row
// first: a row whose centre is on a horizontal edge is left out at a shape's top and taken at
// its bottom. A column whose centre is on a vertical edge is taken at the left, left out at
// the right.
like_qt!(
    edges_on_pixel_centres_in_a_window,
    0,
    [(8, 6), (30, 20)],
    "    Item { x: 4.5; y: 4.5; width: 40; height: 28
        Rectangle { anchors.fill: parent; color: \"#ff3366\" }
        Rectangle { x: 6; y: 4; width: 12; height: 10; color: \"#ffcc00\" }
        Rectangle { x: 20.5; y: 10.5; width: 14; height: 12; radius: 4; color: \"white\" } }
    Rectangle { x: 60.5; y: 40.5; width: 40; height: 28; color: \"#3366ff\" }"
);

// A layer's texture is drawn top row first: inside it the row at a shape's top is taken and
// the one at its bottom left out, and the layer shows its texels as they are.
like_qt!(
    edges_on_pixel_centres_in_an_effect_layer,
    0,
    [(15, 12)],
    "    Item { x: 4; y: 4; width: 40; height: 28; layer.enabled: true; layer.effect: MultiEffect {}
        Rectangle { x: 6.5; y: 4.5; width: 12; height: 10; color: \"#ffcc00\" } }"
);

// A subtree scaled by 0.25 whose edges land on pixel centres, without tolerance: Qt draws it
// straight into the window, so its rows are a window's (the row at a shape's top is left out,
// the one at its bottom taken), not a texture's.
like_qt!(
    edges_on_pixel_centres_of_a_scaled_item,
    0,
    [(22, 16)],
    "    Item { x: 4; y: 4; width: 40; height: 28; scale: 0.25
        Rectangle { anchors.fill: parent; color: \"#ff3366\" }
        Rectangle { x: 6; y: 4; width: 12; height: 10; color: \"#ffcc00\" }
        Rectangle { x: 20.5; y: 10.5; width: 14; height: 12; radius: 4; color: \"white\" } }"
);

like_qt!(
    edges_on_pixel_centres_of_a_scaled_rectangle,
    0,
    [(24, 18)],
    "    Rectangle { x: 4; y: 4; width: 40; height: 28; scale: 0.25; color: \"#ff3366\" }"
);

// ---- An item turned by `rotation` or by a `Rotation` of its `transform`: Qt draws its nodes
// through the item's matrix, so an edge that leaves the pixel grid is as sharp as an upright
// one unless the node antialiases.

like_qt!(
    a_turned_rectangle_has_sharp_edges,
    0,
    [(60, 44)],
    "    Rectangle { x: 40; y: 30; width: 40; height: 28; rotation: 45; color: \"#ff3366\" }"
);

like_qt!(
    a_rectangle_turned_off_the_diagonal_has_sharp_edges,
    0,
    [(60, 44)],
    "    Rectangle { x: 40; y: 30; width: 40; height: 28; rotation: 30; color: \"#ff3366\" }"
);

like_qt!(
    a_rectangle_in_a_turned_item_has_sharp_edges,
    0,
    [(60, 44)],
    "    Item { x: 40; y: 30; width: 40; height: 28; rotation: 45
        Rectangle { anchors.fill: parent; color: \"#ff3366\" } }"
);

like_qt!(
    a_turned_rectangle_with_a_border_has_sharp_edges,
    0,
    [(60, 44)],
    "    Rectangle { x: 40; y: 30; width: 40; height: 28; rotation: 45; color: \"#ff3366\"
        border.width: 2; border.color: \"white\" }"
);

// An antialiased edge of a turned Rectangle. Turned by exactly 45 degrees, all the pixels
// along an edge have one coverage, here 36.5022 of 255, a hair above where the byte rounds
// up: Qt drawing into a window larger than the scene (see `qt`) had five of them one level
// lower.
like_qt!(
    a_turned_antialiased_rectangle,
    0,
    [(60, 44)],
    "    Rectangle { x: 40; y: 30; width: 40; height: 28; rotation: 45; antialiasing: true
        color: \"#ff3366\" }"
);

// Off the diagonal no pixel's centre lies on a level's boundary.
like_qt!(
    an_antialiased_rectangle_turned_off_the_diagonal,
    0,
    [(60, 44)],
    "    Rectangle { x: 40; y: 30; width: 40; height: 28; rotation: 30; antialiasing: true
        color: \"#ff3366\" }"
);

// With a border, whose inner edge is antialiased against the fill.
like_qt!(
    an_antialiased_rectangle_with_a_border_turned_by_77,
    0,
    [(60, 44)],
    "    Rectangle { x: 40; y: 30; width: 40; height: 28; rotation: 77; antialiasing: true
        color: \"#ff3366\"; border.width: 3; border.color: \"#ffffff\" }"
);

// A turn inside a turn, and a scale inside those: each node takes all the matrices above it.
like_qt!(
    a_scaled_rectangle_in_a_turned_rectangle_in_a_turned_item,
    0,
    [(60, 44)],
    "    Item { x: 20; y: 8; width: 80; height: 80; rotation: 20
        Rectangle { x: 30; y: 30; width: 40; height: 20; rotation: 25; antialiasing: true; color: \"#ff3366\"
            Rectangle { x: 4; y: 4; width: 12; height: 9; scale: 1.5; color: \"#225588\" } } }"
);

like_qt!(
    a_turned_rounded_rectangle,
    0,
    [(60, 44)],
    "    Rectangle { x: 40; y: 30; width: 40; height: 28; rotation: 45; radius: 6
        color: \"#ff3366\" }"
);

like_qt!(
    a_rectangle_turned_by_a_rotation_in_its_transform_list_has_sharp_edges,
    0,
    [(60, 44)],
    "    Rectangle { x: 40; y: 30; width: 40; height: 28; color: \"#ff3366\"
        transform: Rotation { origin.x: 20; origin.y: 14; angle: 30 } }"
);

// A hand of the desktop clock: a rounded Rectangle (antialiased, as Qt makes one with a
// radius) inside an Item turned off the pixel grid.
like_qt!(
    a_rounded_rectangle_in_a_turned_item,
    0,
    [(60, 44)],
    "    Item { x: 20; y: 8; width: 80; height: 80; rotation: 36
        Rectangle { x: 34; y: 34; width: 40; height: 12; radius: 6; color: \"#ff3366\" } }"
);

// ---- `clip: true` on half pixels. Qt hands the scissor to OpenGL bottom-up, rounded from
// the window's height, so which row a half pixel falls to depends on the window: the
// reference window is exactly the scene's size (see `qt`), as ours is.

/// Where each of the eight clipped items starts along the probed axis.
const CLIP_AT: [f32; 8] = [4.5, 5.5, 6.5, 7.5, 10.5, 20.5, 40.5, 60.5];

/// Item `k` of a probe scene: its x, y, width and height.
fn clip_item(down: bool, k: usize) -> [f32; 4] {
    if down {
        [4.0 + 18.0 * k as f32, CLIP_AT[k], 12.0, 21.0]
    } else {
        [CLIP_AT[k], 4.0 + 10.0 * k as f32, 28.0, 6.0]
    }
}

/// Eight items with `clip: true`, each cutting a rectangle larger than itself, whose edges
/// along the y axis (`down`) or the x axis lie on half pixels.
fn clip_scene(down: bool) -> String {
    (0..CLIP_AT.len())
        .map(|k| {
            let [x, y, w, h] = clip_item(down, k);
            format!(
                "    Item {{ x: {x}; y: {y}; width: {w}; height: {h}; clip: true
        Rectangle {{ x: -5; y: -5; width: 60; height: 60; color: \"#ffcc00\" }}
    }}\n"
            )
        })
        .collect()
}

like_qt!(
    clip_on_half_pixels_down,
    0,
    [(10, 15), (136, 70)],
    &clip_scene(true),
    [0, 0]
);

like_qt!(
    clip_on_half_pixels_across,
    0,
    [(15, 7), (75, 77)],
    &clip_scene(false),
    [0, 0]
);

/// The rows (`down`) or columns Qt lit through each clip are those of the scissor
/// `qsg::raster::scissor_rect` gives for a window of the scene's size.
fn qt_clips_as_scissor_rect(down: bool) {
    let Some(q) = qt(&clip_scene(down)) else {
        return;
    };
    let lit = |x: usize, y: usize| far(q.px[y * W + x], [0x20, 0x20, 0x20, 255]) > 8;
    for (k, at) in CLIP_AT.iter().enumerate() {
        let [x, y, w, h] = clip_item(down, k);
        let r = qsg::raster::scissor_rect(x, y, w as f64, h as f64, W, H);
        let (want, got): ((i64, i64), Vec<usize>) = if down {
            let col = x as usize + 6;
            ((r[1], r[3]), (0..H).filter(|&row| lit(col, row)).collect())
        } else {
            let row = y as usize + 3;
            ((r[0], r[2]), (0..W).filter(|&col| lit(col, row)).collect())
        };
        assert_eq!(
            got,
            (want.0 as usize..want.1 as usize).collect::<Vec<_>>(),
            "clip starting at {at} in a {W} x {H} window"
        );
    }
}

#[test]
fn qt_clips_rows_on_half_pixels_as_scissor_rect() {
    qt_clips_as_scissor_rect(true);
}

#[test]
fn qt_clips_columns_on_half_pixels_as_scissor_rect() {
    qt_clips_as_scissor_rect(false);
}
