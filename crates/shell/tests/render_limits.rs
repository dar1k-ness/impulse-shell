//! Scenes whose QML asks for something no screen can show (an effect that is its own source,
//! sizes and blur parameters of 1e9, a negative blur radius, Shape coordinates of 1e9): each
//! one paints in bounded time and memory on the CPU and the GPU path, and the rest of the
//! window is drawn as usual.

use qsg::raster::Canvas;
use shell::scene::{self, Target};

const W: usize = 160;
const H: usize = 64;
const MARK: [u8; 4] = [0, 255, 0, 255];
/// The texture size limit of the GPU path here. Mesa's software driver allows 16384 texels a
/// side: a layer of 1e9 clamped to that is 1 GiB, which it fills and blurs in seconds. The
/// clamp is the same code at any limit; `layer_past_the_texture_size_limit_draws_scaled_down`
/// (gpu.rs) runs it at the driver's.
const MAX_TEX: usize = 2048;

/// One window: `body` on the left and a green marker in the bottom right corner, after it in
/// paint order.
fn window(imports: &str, body: &str) -> String {
    format!(
        "import QtQuick\nimport Quickshell\n{imports}\nFloatingWindow {{\n    width: {W}; height: {H}\n    \
         color: \"#202020\"\n{body}\n    Rectangle {{ x: {}; y: {}; width: 8; height: 8; color: \"#00ff00\" }}\n}}\n",
        W - 8,
        H - 8
    )
}

/// Caps this process's address space: an allocation QML sizes fails at once instead of
/// swapping the machine.
fn limit_memory() {
    let lim = libc::rlimit {
        rlim_cur: 6 << 30,
        rlim_max: 6 << 30,
    };
    // SAFETY: `lim` is a valid rlimit; RLIMIT_AS only bounds later allocations.
    assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_AS, &lim) }, 0);
}

fn paint(qml: &str, gpu: bool) -> Canvas {
    let mut rt = qml::runtime::Runtime::new(None);
    let win = rt.load_str(qml).expect("scene loads");
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
    g.cap_texture_size(MAX_TEX);
    let mut fr = shell::gpu::Frame::new(W, H);
    scene::paint_window(&mut rt, &mut fonts, win, &mut Target::Gpu(&mut fr));
    let mut s = g.pbuffer(W as i32, H as i32).expect("pbuffer");
    s.reswap();
    g.render(&mut s, &mut fr, || {});
    g.read_surface(&s)
}

/// Paints `qml` on one path and checks the marker drawn after the scene's body. A scene that
/// never returns is ended by nextest's slow-timeout for this binary (.config/nextest.toml).
fn bounded(qml: String, gpu: bool) -> Canvas {
    limit_memory();
    let c = paint(&qml, gpu);
    assert_eq!(
        c.px[(H - 4) * W + W - 4],
        MARK,
        "the item after the scene is drawn"
    );
    c
}

fn effects(body: &str, gpu: bool) -> Canvas {
    bounded(
        window(
            "import QtQuick.Effects\nimport QtQuick.Shapes\nimport Qt5Compat.GraphicalEffects",
            body,
        ),
        gpu,
    )
}

/// The scene `$body` as two tests, `$name::cpu` and `$name::gpu`.
macro_rules! both_paths {
    ($name:ident, $body:expr) => {
        mod $name {
            use super::*;

            #[test]
            fn cpu() {
                effects(&$body, false);
            }
            #[test]
            fn gpu() {
                effects(&$body, true);
            }
        }
    };
}

const SRC: &str =
    "    Rectangle { id: src; x: 4; y: 4; width: 40; height: 40; radius: 8; color: \"#ff3366\" }\n";

// ---- An effect whose source is itself, another effect that sources it, or its ancestor.

both_paths!(
    blur_of_itself_paints,
    "    GaussianBlur { id: g; x: 4; y: 4; width: 40; height: 40; source: g; radius: 4; samples: 9 }"
);

both_paths!(
    two_blurs_of_each_other_paint,
    "    GaussianBlur { id: a; x: 4; y: 4; width: 40; height: 40; source: b; radius: 4; samples: 9 }
    GaussianBlur { id: b; x: 50; y: 4; width: 40; height: 40; source: a; radius: 4; samples: 9 }"
);

both_paths!(
    multi_effect_of_itself_paints,
    "    MultiEffect { id: g; x: 4; y: 4; width: 40; height: 40; blurEnabled: true; blur: 0.5; source: g }"
);

const PARENT_BLUR: &str = "    Item { id: p; x: 4; y: 4; width: 40; height: 40
        Rectangle { width: 20; height: 20; color: \"#ff3366\" }
        GaussianBlur { anchors.fill: parent; source: p; radius: 4; samples: 9 }
    }";

mod blur_of_its_parent_paints {
    use super::*;

    #[test]
    fn cpu() {
        // The parent's own content is still there.
        assert_eq!(effects(PARENT_BLUR, false).px[6 * W + 6][3], 255);
    }

    #[test]
    fn gpu() {
        assert_eq!(effects(PARENT_BLUR, true).px[6 * W + 6][3], 255);
    }
}

both_paths!(
    shadow_and_overlay_of_their_parent_paint,
    "    Item { id: p; x: 4; y: 4; width: 40; height: 40
        Rectangle { width: 20; height: 20; color: \"#ff3366\" }
        DropShadow { anchors.fill: parent; source: p; radius: 4; samples: 9; transparentBorder: true }
        ColorOverlay { anchors.fill: parent; source: p; color: \"#8000ff00\" }
        OpacityMask { anchors.fill: parent; source: p; maskSource: p }
        Desaturate { anchors.fill: parent; source: p; desaturation: 0.5 }
    }"
);

both_paths!(
    layer_effect_sourcing_its_own_item_paints,
    "    Rectangle { id: p; x: 4; y: 4; width: 40; height: 40; color: \"#ff3366\"
        layer.enabled: true
        layer.effect: MultiEffect { source: p; blurEnabled: true; blur: 0.5 }
    }"
);

// ---- Sizes and blur parameters far beyond any screen.

both_paths!(
    huge_layer_paints,
    "    Rectangle { x: 4; y: 4; width: 1e9; height: 1e9; color: \"#ff3366\"
        layer.enabled: true
        layer.effect: OpacityMask { maskSource: Rectangle { width: 1e9; height: 1e9; radius: 8 } }
    }"
);

both_paths!(
    huge_blur_samples_paint,
    format!(
        "{SRC}    GaussianBlur {{ x: 4; y: 4; width: 40; height: 40; source: src; radius: 4; samples: 1e9 }}
    DropShadow {{ x: 50; y: 4; width: 40; height: 40; source: src; radius: 4; samples: 1e9 }}"
    )
);

both_paths!(
    huge_blur_radius_paints,
    format!(
        "{SRC}    GaussianBlur {{ x: 4; y: 4; width: 40; height: 40; source: src; radius: 1e9; samples: 9; transparentBorder: true }}
    DropShadow {{ x: 50; y: 4; width: 40; height: 40; source: src; radius: 1e9; samples: 9; transparentBorder: true }}"
    )
);

both_paths!(
    huge_radius_with_matching_samples_paints,
    format!(
        "{SRC}    GaussianBlur {{ x: 4; y: 4; width: 40; height: 40; source: src; radius: 100000; samples: 200001; transparentBorder: true }}"
    )
);

// The original's lock screen blur (modules/rusty/background/Background.qml): a Loader's GaussianBlur whose
// radius comes from the user's config (`lock.blur.radius`) and whose samples follow it.
both_paths!(
    lock_blur_radius_from_config_paints,
    format!(
        "{SRC}    property real configured: 1e9
    Loader {{ x: 4; y: 4; width: 40; height: 40
        sourceComponent: GaussianBlur {{ source: src; radius: configured; samples: radius * 2 + 1 }}
    }}"
    )
);

both_paths!(
    huge_blur_and_shadow_items_paint,
    format!(
        "{SRC}    GaussianBlur {{ x: 4; y: 4; width: 1e9; height: 1e9; source: src; radius: 4; samples: 9 }}
    DropShadow {{ x: 4; y: 4; width: 1e9; height: 1e9; source: src; radius: 4; samples: 9 }}"
    )
);

both_paths!(
    huge_multi_effect_paints,
    format!(
        "{SRC}    MultiEffect {{ x: 4; y: 4; width: 1e9; height: 1e9; source: src; blurEnabled: true; blur: 0.5 }}
    MultiEffect {{ x: 4; y: 4; width: 1e9; height: 1e9; source: src; shadowEnabled: true }}"
    )
);

both_paths!(
    huge_multisampled_shape_layer_paints,
    "    Shape { x: 4; y: 4; width: 1e9; height: 1e9
        layer.enabled: true; layer.samples: 4
        ShapePath { fillColor: \"#ff3366\"; strokeColor: \"white\"; strokeWidth: 2
            startX: 2; startY: 2
            PathLine { x: 30; y: 2 } PathLine { x: 30; y: 30 } PathLine { x: 2; y: 2 }
        }
    }"
);

both_paths!(
    huge_blur_source_paints,
    "    Rectangle { id: big; width: 60000; height: 60000; color: \"#ff3366\"; visible: false }
    GaussianBlur { x: 4; y: 4; width: 40; height: 40; source: big; radius: 4; samples: 9 }
    DropShadow { x: 50; y: 4; width: 40; height: 40; source: big; radius: 4; samples: 9 }"
);

// ---- A negative blur radius.

fn negative_radius() -> String {
    format!(
        "{SRC}    GaussianBlur {{ x: 50; y: 4; width: 40; height: 40; source: src; radius: -5; samples: 9; transparentBorder: true }}
    DropShadow {{ x: 100; y: 4; width: 40; height: 40; source: src; radius: -5; samples: 9; transparentBorder: true }}"
    )
}

#[test]
fn negative_blur_radius_paints_on_the_cpu() {
    let cpu = effects(&negative_radius(), false);
    // Both effects drew their source.
    for x in [70, 120] {
        assert_eq!(cpu.px[24 * W + x], [255, 51, 102, 255], "x {x}");
    }
}

#[test]
fn negative_blur_radius_paints_on_the_cpu_like_the_gpu() {
    let (cpu, gpu) = (
        effects(&negative_radius(), false),
        effects(&negative_radius(), true),
    );
    let worst = cpu
        .px
        .iter()
        .zip(&gpu.px)
        .map(|(a, b)| (0..4).map(|k| a[k].abs_diff(b[k])).max().unwrap())
        .max()
        .unwrap();
    assert!(worst <= 3, "CPU and GPU differ by {worst}");
}

// ---- Shape coordinates far outside the window. Unclipped, the outlines at 1e9 took 15 s to
// fill and the one at 1e10 ten times that.

fn far_shape(gpu: bool) -> Canvas {
    bounded(
        window(
            "import QtQuick.Shapes",
            "    Shape { x: 4; y: 4; width: 60; height: 40
        ShapePath { fillColor: \"#ff3366\"; strokeColor: \"white\"; strokeWidth: 2
            startX: 1e9; startY: 0
            PathLine { x: 24; y: 1e9 } PathLine { x: 1e9; y: 14 }
        }
    }
    Shape { x: 70; y: 4; width: 60; height: 40; preferredRendererType: Shape.CurveRenderer
        ShapePath { fillColor: \"#ff3366\"; strokeColor: \"white\"; strokeWidth: 2
            startX: 1e9; startY: 0
            PathLine { x: 24; y: 1e9 } PathLine { x: 1e9; y: 14 }
        }
    }
    Shape { x: 4; y: 4; width: 60; height: 40
        ShapePath { fillColor: \"#ff3366\"; strokeColor: \"white\"; strokeWidth: 2
            startX: 1e10; startY: 0
            PathLine { x: 24; y: 1e10 } PathLine { x: 1e10; y: 14 }
        }
    }",
        ),
        gpu,
    )
}

#[test]
fn shape_with_far_coordinates_paints_in_time_on_the_cpu() {
    far_shape(false);
}

#[test]
fn shape_with_far_coordinates_paints_in_time_on_the_gpu() {
    far_shape(true);
}

// ---- The same items in Qt 6.12 (qmltestrunner on OpenGL, pixels read with grabImage).

fn like_qt(gpu: bool) {
    let c = effects(
        &format!(
            "{SRC}    GaussianBlur {{ id: own; x: 50; y: 4; width: 20; height: 20; source: own; radius: 4; samples: 9 }}
    Item {{
        id: par; x: 74; y: 4; width: 20; height: 20
        Rectangle {{ anchors.fill: parent; color: \"#3366ff\" }}
        GaussianBlur {{ anchors.fill: parent; source: par; radius: 4; samples: 9 }}
    }}
    GaussianBlur {{ x: 100; y: 4; width: 20; height: 20; source: src; radius: -6; samples: 9; transparentBorder: true }}
    Shape {{
        x: 124; y: 4; width: 30; height: 30
        ShapePath {{
            fillColor: \"#00ffff\"; strokeColor: \"transparent\"
            startX: 1e9; startY: 0
            PathLine {{ x: 12; y: 1e9 }}
            PathLine {{ x: 1e9; y: 14 }}
        }}
    }}"
        ),
        gpu,
    );
    let px = |x: usize, y: usize| c.px[y * W + x];
    const BG: [u8; 4] = [0x20, 0x20, 0x20, 255];
    assert_eq!(px(60, 14), BG, "a blur of itself draws nothing");
    assert_eq!(
        px(84, 14),
        [0x33, 0x66, 0xff, 255],
        "a blur of its parent shows the parent"
    );
    assert_eq!(
        (px(110, 14), px(97, 14)),
        ([255, 51, 102, 255], BG),
        "a negative radius draws the source and no border"
    );
    for (x, y) in [(126, 6), (139, 19), (152, 32)] {
        assert_eq!(px(x, y), BG, "the far sliver covers nothing at {x},{y}");
    }
}

#[test]
fn cycles_negative_radius_and_far_shape_paint_like_qt_on_the_cpu() {
    like_qt(false);
}

#[test]
fn cycles_negative_radius_and_far_shape_paint_like_qt_on_the_gpu() {
    like_qt(true);
}

// ---- A blur with more taps than the shader holds as constants (241: the tap loop), in
// Qt 6.12.0 (qmltestrunner on OpenGL, llvmpipe, pixels read with grabImage).

/// Three stripes blurred across each other with radius 120 and 241 samples: over the item
/// (the edge texels repeat past it) and, clipped to a strip, with a transparent border.
const LOOPED_BLUR: &str = "    Item {
        id: stripes; x: 4; y: 4; width: 60; height: 40
        Rectangle { width: 20; height: 40; color: \"#ff3366\" }
        Rectangle { x: 20; width: 20; height: 40; color: \"#3366ff\" }
        Rectangle { x: 40; width: 20; height: 40; color: \"#00ff00\" }
    }
    GaussianBlur { x: 70; y: 4; width: 60; height: 40; source: stripes; radius: 120; samples: 241 }
    Item {
        x: 70; y: 46; width: 80; height: 16; clip: true
        GaussianBlur { x: 10; y: -12; width: 60; height: 40; source: stripes; radius: 120; samples: 241; transparentBorder: true }
    }";

const BLUR_OVER_ITEM: [(usize, usize, [u8; 4]); 9] = [
    (70, 24, [188, 87, 112, 255]),
    (80, 24, [163, 103, 110, 255]),
    (90, 24, [137, 122, 104, 255]),
    (100, 24, [110, 143, 95, 255]),
    (110, 24, [84, 165, 82, 255]),
    (120, 24, [61, 186, 68, 255]),
    (129, 24, [43, 203, 54, 255]),
    (100, 5, [110, 143, 95, 255]),
    (100, 43, [110, 143, 95, 255]),
];

const SOURCE_BORDERED_BLUR_AND_AROUND: [(usize, usize, [u8; 4]); 16] = [
    (10, 24, [255, 51, 102, 255]),
    (30, 24, [51, 102, 255, 255]),
    (50, 24, [0, 255, 0, 255]),
    (72, 54, [50, 43, 49, 255]),
    (80, 54, [52, 47, 52, 255]),
    (90, 54, [52, 51, 54, 255]),
    (100, 54, [51, 54, 55, 255]),
    (110, 54, [49, 57, 55, 255]),
    (120, 54, [45, 59, 53, 255]),
    (130, 54, [42, 58, 50, 255]),
    (140, 54, [38, 57, 46, 255]),
    (148, 54, [36, 54, 43, 255]),
    (110, 47, [48, 57, 55, 255]),
    (110, 61, [48, 57, 54, 255]),
    (152, 54, [32, 32, 32, 255]),
    (135, 24, [32, 32, 32, 255]),
];

/// The pixels of `c` further than `levels` from Qt's.
fn unlike_qt(c: &Canvas, qt: &[(usize, usize, [u8; 4])], levels: u8) -> Vec<String> {
    let far = |&(x, y, qt): &(usize, usize, [u8; 4])| {
        let got = c.px[y * W + x];
        (0..4)
            .any(|i| got[i].abs_diff(qt[i]) > levels)
            .then(|| format!("({x}, {y}): {got:?}, Qt has {qt:?}"))
    };
    qt.iter().filter_map(far).collect()
}

/// Two passes, each rounded to 8 bits: one level per pass.
#[test]
fn looped_blur_paints_like_qt_on_the_gpu() {
    let c = effects(LOOPED_BLUR, true);
    let mut wrong = unlike_qt(&c, &BLUR_OVER_ITEM, 2);
    wrong.extend(unlike_qt(&c, &SOURCE_BORDERED_BLUR_AND_AROUND, 2));
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// The CPU path runs the same taps where the border is transparent. Without one it blurs a
/// source reduced `radius / 16` times (`scene.rs`, GaussianBlur) and scales the result back:
/// here 7 times, which moves these pixels by up to 11 levels.
#[test]
fn looped_blur_paints_like_qt_on_the_cpu() {
    let c = effects(LOOPED_BLUR, false);
    let mut wrong = unlike_qt(&c, &SOURCE_BORDERED_BLUR_AND_AROUND, 2);
    wrong.extend(unlike_qt(&c, &BLUR_OVER_ITEM, 2));
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
