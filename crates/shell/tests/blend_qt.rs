//! One colour drawn over another is blended as the video card blends it, not as QPainter does.
//! 256 opaque grey rows under 256 black columns of every alpha give every pair of bytes
//! (what is under, how much of it is kept) once. Qt Quick through OpenGL on llvmpipe
//! (support/qt_gl.rs) rounds `under * kept / 255` to nearest in all 65536, which is
//! `mul255::nearest`; Qt's `qt_div_255`, which QPainter blends with, is one
//! less in 24 of them, 253 under 64 of 255 black among them (190, not 189).
//!
//! What Qt does before the video card has the picture is on the processor and is
//! `qt_div_255`: a PNG of every grey at every alpha is premultiplied by `qPremultiply` when
//! Qt Quick makes a texture of it, and over black the window shows that product itself,
//! `mul255::qt` in all 65536.

#[path = "support/qt_gl.rs"]
mod qt_gl;

use mul255::{nearest, qt as qt_div_255};
use qsg::raster::Canvas;
use shell::scene::{self, Target};

const SOURCE: &str = "import QtQuick
Item {
    width: 256; height: 256
    Repeater {
        model: 256
        Rectangle {
            required property int index
            y: index; width: 256; height: 1
            color: Qt.rgba(index / 255, index / 255, index / 255, 1)
        }
    }
    Repeater {
        model: 256
        Rectangle {
            required property int index
            x: index; width: 1; height: 256
            color: Qt.rgba(0, 0, 0, index / 255)
        }
    }
}
";

fn ours(source: &str) -> Canvas {
    let mut rt = qml::runtime::Runtime::new(None);
    let mut fonts = scene::Fonts::default();
    let root = rt.load_str(source).unwrap();
    let mut picture = Canvas::new(256, 256, [0, 0, 0, 0]);
    // A Canvas paints in the frame after the one that laid it out.
    for _ in 0..3 {
        rt.settle();
        scene::layout_window(&mut rt, &mut fonts, root, [256.0, 256.0]);
        rt.canvases_available();
        rt.settle();
        scene::paint_window(&mut rt, &mut fonts, root, &mut Target::Cpu(&mut picture));
    }
    let errors = rt.world.borrow().errors.clone();
    assert!(errors.is_empty(), "script errors: {errors:?}");
    picture
}

/// How many of the 65536 pixels are not the grey `value(row, column)`.
fn unlike(picture: &Canvas, value: impl Fn(u32, u32) -> u32) -> usize {
    assert_eq!((picture.w, picture.h), (256, 256));
    (0..256 * 256)
        .filter(|i| {
            let v = value((i / 256) as u32, (i % 256) as u32) as u8;
            picture.px[*i] != [v, v, v, 255]
        })
        .count()
}

fn differ(ours: &Canvas, qt: &Canvas) -> usize {
    (0..256 * 256).filter(|i| ours.px[*i] != qt.px[*i]).count()
}

#[test]
fn every_pair_of_bytes_is_blended_as_qt_quick_blends_it() {
    if !qt_gl::available() {
        return;
    }
    let (_, qt) = qt_gl::run(SOURCE);
    let kept = |formula: fn(u32, u32) -> u32| move |grey, alpha| formula(grey, 255 - alpha);
    assert_eq!(unlike(&qt, kept(nearest)), 0, "Qt Quick through OpenGL");
    assert_eq!(unlike(&qt, kept(qt_div_255)), 24, "Qt Quick through OpenGL");
    assert_eq!(qt.px[253 * 256 + 64], [190, 190, 190, 255]);
    assert_eq!(
        differ(&ours(SOURCE), &qt),
        0,
        "of 65536 pixels differ from Qt"
    );
}

#[test]
fn every_pair_of_bytes_is_premultiplied_as_qt_premultiplies_it() {
    if !qt_gl::available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("pairs.png");
    let px = (0..256 * 256).map(|i| [(i / 256) as u8, (i % 256) as u8]);
    let px = px.map(|[grey, alpha]| [grey, grey, grey, alpha]).collect();
    Canvas::from_px(256, 256, px)
        .save_png(png.to_str().unwrap())
        .unwrap();
    let source = format!(
        "import QtQuick\nRectangle {{\n    width: 256; height: 256; color: \"black\"\n    \
         Image {{ source: \"file://{}\" }}\n}}\n",
        png.display()
    );
    let (_, qt) = qt_gl::run(&source);
    assert_eq!(unlike(&qt, qt_div_255), 0, "Qt Quick through OpenGL");
    assert_eq!(unlike(&qt, nearest), 24, "Qt Quick through OpenGL");
    assert_eq!(qt.px[253 * 256 + 191], [189, 189, 189, 255]);
    assert_eq!(
        differ(&ours(&source), &qt),
        0,
        "of 65536 pixels differ from Qt"
    );
}

/// `putImageData` of every grey at every alpha: Canvas premultiplies the picture on the
/// processor (`qPremultiply`) and QPainter draws it, the window over black shows the product.
#[test]
fn every_pair_of_bytes_of_image_data_is_premultiplied_as_qt_premultiplies_it() {
    if !qt_gl::available() {
        return;
    }
    let source = "import QtQuick
Rectangle {
    width: 256; height: 256; color: \"black\"
    Canvas {
        anchors.fill: parent
        onPaint: {
            var ctx = getContext(\"2d\");
            var picture = ctx.createImageData(256, 256);
            var d = picture.data;
            for (var i = 0; i < 65536; i++) {
                var grey = i >> 8;
                d[4 * i] = grey; d[4 * i + 1] = grey; d[4 * i + 2] = grey; d[4 * i + 3] = i & 255;
            }
            ctx.putImageData(picture, 0, 0, 0, 0, 256, 256);
        }
    }
}
";
    let (_, qt) = qt_gl::run(source);
    assert_eq!(unlike(&qt, qt_div_255), 0, "Qt Quick through OpenGL");
    assert_eq!(unlike(&qt, nearest), 24, "Qt Quick through OpenGL");
    assert_eq!(qt.px[253 * 256 + 191], [189, 189, 189, 255]);
    assert_eq!(
        differ(&ours(source), &qt),
        0,
        "of 65536 pixels differ from Qt"
    );
}

/// An animated WebP whose `frames` are each one picture of 256x256 RGBA: the container around
/// the VP8L chunk `image` encodes for a frame, each drawn without blending for one second.
fn animated_webp(frames: &[Vec<u8>]) -> Vec<u8> {
    let u24 = |v: u32| v.to_le_bytes()[..3].to_vec();
    let chunk = |tag: &[u8; 4], body: &[u8]| {
        let mut c = tag.to_vec();
        c.extend((body.len() as u32).to_le_bytes());
        c.extend(body);
        c.resize(c.len() + body.len() % 2, 0);
        c
    };
    let mut body = b"WEBP".to_vec();
    // VP8X: animation and alpha flags, canvas size minus one. ANIM: background, loop forever.
    body.extend(chunk(
        b"VP8X",
        &[vec![0x12, 0, 0, 0], u24(255), u24(255)].concat(),
    ));
    body.extend(chunk(b"ANIM", &[0; 6]));
    for frame in frames {
        let mut still = Vec::new();
        image::codecs::webp::WebPEncoder::new_lossless(&mut still)
            .encode(frame, 256, 256, image::ExtendedColorType::Rgba8)
            .unwrap();
        assert_eq!(&still[12..16], b"VP8L");
        // ANMF: offset, size minus one, duration, no blending; then the frame's chunk.
        let head = [u24(0), u24(0), u24(255), u24(255), u24(1000), vec![2]].concat();
        body.extend(chunk(b"ANMF", &[&head[..], &still[12..]].concat()));
    }
    let mut riff = b"RIFF".to_vec();
    riff.extend((body.len() as u32).to_le_bytes());
    riff.extend(body);
    riff
}

/// A frame of an animated picture is premultiplied on the processor like a still one: QMovie
/// composes the frame of a WebP with QPainter into a premultiplied QImage.
#[test]
fn every_pair_of_bytes_of_an_animated_frame_is_premultiplied_as_qt_premultiplies_it() {
    if !qt_gl::available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let webp = dir.path().join("pairs.webp");
    let pairs = (0..256 * 256).flat_map(|i| {
        let [grey, alpha] = [(i / 256) as u8, (i % 256) as u8];
        [grey, grey, grey, alpha]
    });
    // The second frame is opaque white: a player that shows it instead has every pixel wrong.
    let frames = [pairs.collect(), vec![255; 256 * 256 * 4]];
    std::fs::write(&webp, animated_webp(&frames)).unwrap();
    let source = format!(
        "import QtQuick\nRectangle {{\n    width: 256; height: 256; color: \"black\"\n    \
         AnimatedImage {{ source: \"file://{}\"; playing: false }}\n}}\n",
        webp.display()
    );
    let (_, qt) = qt_gl::run(&source);
    assert_eq!(unlike(&qt, qt_div_255), 0, "Qt Quick through OpenGL");
    assert_eq!(unlike(&qt, nearest), 24, "Qt Quick through OpenGL");
    assert_eq!(qt.px[253 * 256 + 191], [189, 189, 189, 255]);
    assert_eq!(
        differ(&ours(&source), &qt),
        0,
        "of 65536 pixels differ from Qt"
    );
}

/// 256 mask icons of 16x16, one per `index`, each over the same file of the 256 alphas: the
/// pixel of icon `index` at alpha `a` is at [`mask_pixel`].
fn mask_icons(file: &std::path::Path, under: &str, color: &str) -> String {
    format!(
        "import QtQuick
import org.kde.kirigami as Kirigami
Rectangle {{
    width: 256; height: 256; color: \"{under}\"
    Repeater {{
        model: 256
        Kirigami.Icon {{
            required property int index
            x: (index % 16) * 16; y: (index >> 4) * 16; width: 16; height: 16
            source: \"file://{}\"; isMask: true
            color: {color}
        }}
    }}
}}
",
        file.display()
    )
}

fn mask_pixel(index: usize, alpha: usize) -> usize {
    ((index / 16) * 16 + alpha / 16) * 256 + (index % 16) * 16 + alpha % 16
}

/// Kirigami.Icon's `isMask` is a shader (Kirigami 6.31, src/primitives/shaders/icon.frag):
/// the colour times the icon's alpha is the video card's product, rounded to nearest, not
/// QPainter's. With a colour that is not opaque the shader keeps the icon's alpha as the
/// alpha: a transparent colour over an opaque mask paints black, not nothing.
#[test]
fn every_pair_of_bytes_of_a_mask_icon_is_tinted_as_kirigami_tints_it() {
    if !qt_gl::available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("mask.png");
    let px = (0..=255).map(|alpha| [40, 90, 160, alpha]).collect();
    Canvas::from_px(16, 16, px)
        .save_png(png.to_str().unwrap())
        .unwrap();

    // Every grey, opaque, over black: the pixel is the grey times the mask's alpha.
    let grey = mask_icons(
        &png,
        "black",
        "Qt.rgba(index / 255, index / 255, index / 255, 1)",
    );
    let (_, qt) = qt_gl::run(&grey);
    let unlike = |formula: fn(u32, u32) -> u32| {
        let pairs = (0..256 * 256).map(|i| (i / 256, i % 256));
        pairs
            .filter(|(grey, alpha)| {
                let v = formula(*grey as u32, *alpha as u32) as u8;
                qt.px[mask_pixel(*grey, *alpha)] != [v, v, v, 255]
            })
            .count()
    };
    assert_eq!(unlike(nearest), 0, "Kirigami through OpenGL");
    assert_eq!(unlike(qt_div_255), 24, "Kirigami through OpenGL");
    assert_eq!(qt.px[mask_pixel(253, 191)], [190, 190, 190, 255]);
    assert_eq!(
        differ(&ours(&grey), &qt),
        0,
        "of 65536 pixels differ from Qt"
    );

    // One colour at every alpha, over white.
    let clear = mask_icons(
        &png,
        "white",
        "Qt.rgba(200 / 255, 100 / 255, 253 / 255, index / 255)",
    );
    let (_, qt) = qt_gl::run(&clear);
    assert_eq!(qt.px[mask_pixel(0, 255)], [0, 0, 0, 255]);
    assert_eq!(qt.px[mask_pixel(128, 128)], [177, 152, 191, 255]);
    let ours = ours(&clear);
    // llvmpipe's product of the three floats is one off the nearest byte in a channel of 98
    // of the 65536 pixels.
    let off = (0..256 * 256).filter(|i| ours.px[*i] != qt.px[*i]);
    let off: Vec<_> = off.map(|i| (ours.px[i], qt.px[i])).collect();
    let by_one = |(a, b): &([u8; 4], [u8; 4])| (0..4).all(|c| a[c].abs_diff(b[c]) <= 1);
    assert!(off.len() <= 98 && off.iter().all(by_one), "{off:?}");
}

/// A mask icon stretched over its item: the video card filters the pixmap and icon.frag tints
/// what the filter gives, so the colour is multiplied by the filtered alpha and rounded once.
/// A pixmap tinted before it is stretched has the rounded products filtered instead, which is
/// one off in about half of the pixels.
#[test]
fn a_stretched_mask_icon_is_tinted_after_it_is_filtered() {
    if !qt_gl::available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("mask.png");
    let px = (0..256).map(|i| [40, 90, 160, ((i % 16) * 37 + (i / 16) * 91) as u8]);
    Canvas::from_px(16, 16, px.collect())
        .save_png(png.to_str().unwrap())
        .unwrap();
    let icon = |at: &str, color: &str| {
        format!(
            "    Kirigami.Icon {{ {at}; roundToIconSize: false; isMask: true; color: {color}; \
             source: \"file://{}\" }}\n",
            png.display()
        )
    };
    let source = format!(
        "import QtQuick\nimport org.kde.kirigami as Kirigami\nRectangle {{\n    \
         width: 256; height: 256; color: \"#202020\"\n{}{}{}{}{}}}\n",
        icon("width: 32; height: 32", "\"#ff8040\""),
        icon("x: 40; width: 24; height: 24", "\"#ff8040\""),
        icon("x: 80; width: 40; height: 20", "\"#ff8040\""),
        icon("x: 130; width: 16; height: 16", "\"#ff8040\""),
        icon("y: 60; width: 100; height: 100", "\"#3070c0\""),
    );
    let (_, qt) = qt_gl::run(&source);
    // The icons are there: the first is not the background.
    assert_ne!(qt.px[8 * 256 + 8], [0x20, 0x20, 0x20, 255]);
    let ours = ours(&source);
    let off = (0..256 * 256).filter(|i| ours.px[*i] != qt.px[*i]);
    let off: Vec<_> = off
        .map(|i| (i % 256, i / 256, ours.px[i], qt.px[i]))
        .collect();
    assert!(
        off.is_empty(),
        "{} of 65536 pixels differ from Qt, the first (x, y, ours, Qt): {:?}",
        off.len(),
        &off[..off.len().min(12)]
    );
}
