//! AnimatedImage against Qt 6.12 itself. QImageReader reads every frame of animated GIF and
//! WebP files (their count, delays, sizes and pixels, at `sourceSize` too) and the test
//! compares them with `shell::movie::Movie`; then Qt Quick shows an AnimatedImage whose
//! `currentFrame` is stepped forward, back and out of range, and the test compares what its
//! window shows, `currentFrame` and `playing` with the shell's scene; and it plays files that
//! ask for a number of rounds under each `loops` and `finishBehavior`, and the test compares
//! the order of `currentFrameChanged`, `playingChanged` and `finished` with the shell's.
use shell::movie::Movie;
use std::path::Path;

/// The PySide6 of the testenv image (tools/testenv/Containerfile).
const PYTHON: &str = "/opt/qv4-venv/bin/python";

/// Per `path width height` argument (the scaled size, 0 for none) prints
/// `imageCount frame...`, a frame being `delay:width:height:crc32:hex:rgb32`: of its
/// RGBA8888_Premultiplied bytes (`-` for the hex of a big frame), and whether the picture
/// has no alpha channel (Format_RGB32). Such a picture's bytes are the ones it holds.
const READER: &str = r#"
import sys, zlib
from PySide6.QtCore import QSize
from PySide6.QtGui import QGuiApplication, QImage, QImageReader
app = QGuiApplication(sys.argv[:1])
for arg in sys.argv[1:]:
    path, sw, sh = arg.rsplit(" ", 2)
    r = QImageReader(path)
    if int(sw) or int(sh):
        r.setScaledSize(QSize(int(sw), int(sh)))
    out = [str(r.imageCount())]
    while r.canRead():
        im = r.read()
        if im.isNull():
            break
        if im.format() == QImage.Format_RGB32:
            # The four bytes of each pixel as the image holds them: the reorder to RGBA
            # multiplies nothing, where a conversion from Format_RGB32 would set alpha to 255.
            px = QImage(im.constBits(), im.width(), im.height(), im.bytesPerLine(), QImage.Format_ARGB32_Premultiplied)
        else:
            px = im
        px = px.convertToFormat(QImage.Format_RGBA8888_Premultiplied)
        raw = bytes(px.constBits())
        hexed = raw.hex() if len(raw) <= 4096 else "-"
        rgb32 = im.format() == QImage.Format_RGB32
        out.append("%d:%d:%d:%d:%s:%d" % (r.nextImageDelay(), im.width(), im.height(), zlib.crc32(raw), hexed, rgb32))
    print(" ".join(out))
"#;

/// What Qt is asked for. Pixels a QImageReader reads, properties and signals are the work of
/// the processor and need no window on a screen. The pixels of a window are Qt Quick's, which
/// draws a user's scene through OpenGL: its software renderer (all `offscreen` has) answers
/// differently in places, so a window is taken from OpenGL on llvmpipe under the test
/// compositor.
#[derive(Clone, Copy)]
enum Asked {
    NoWindow,
    #[cfg(feature = "interp")]
    Window,
}

/// The PySide6 that runs `script`.
fn python(script: &str, asked: Asked) -> std::process::Command {
    let mut run = std::process::Command::new(PYTHON);
    run.args(["-c", script]);
    match asked {
        Asked::NoWindow => run.env("QT_QPA_PLATFORM", "offscreen"),
        #[cfg(feature = "interp")]
        Asked::Window => run
            .env("QT_QPA_PLATFORM", "wayland-egl")
            .env("LIBGL_ALWAYS_SOFTWARE", "1"),
    };
    run
}

/// The lines `script` prints for `args`; `None` (reported) without PySide6.
fn qt(script: &str, asked: Asked, args: &[String]) -> Option<Vec<String>> {
    if !Path::new(PYTHON).exists() {
        testenv::equiv_missing(format_args!("{PYTHON} (PySide6)"));
        return None;
    }
    let out = python(script, asked).args(args).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{text}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    assert_eq!(lines.len(), args.len(), "{text}");
    Some(lines)
}

fn hex(px: &[[u8; 4]]) -> String {
    px.as_flattened()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn crc(px: &[[u8; 4]]) -> u32 {
    let mut c = flate2::Crc::new();
    c.update(px.as_flattened());
    c.sum()
}

const PALETTE: [[u8; 3]; 8] = [
    [255, 0, 0],
    [0, 255, 0],
    [0, 0, 255],
    [255, 255, 0],
    [0, 255, 255],
    [255, 0, 255],
    [40, 40, 40],
    [250, 250, 250],
];

/// One image of a GIF: its rectangle, delay (1/100 s), transparent index and disposal.
#[derive(Clone, Copy)]
struct Part {
    rect: [u16; 4],
    delay: u16,
    trans: Option<u8>,
    dispose: gif::DisposalMethod,
    /// A colour table of its own (the global one reversed).
    local: bool,
    interlaced: bool,
}

fn part(rect: [u16; 4], delay: u16) -> Part {
    Part {
        rect,
        delay,
        trans: None,
        dispose: gif::DisposalMethod::Any,
        local: false,
        interlaced: false,
    }
}

/// A looping GIF on a `w`x`h` screen with background index `bg`; pixel (x, y) of image `k`
/// has colour index `(x + y + k) % 8`.
fn gif(w: u16, h: u16, bg: u8, parts: &[Part]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = gif::Encoder::new(&mut out, w, h, PALETTE.as_flattened()).unwrap();
        enc.set_repeat(gif::Repeat::Infinite).unwrap();
        for (k, p) in parts.iter().enumerate() {
            let [left, top, width, height] = p.rect;
            let index = |i: u16| ((i % width + i / width + k as u16) % 8) as u8;
            let reversed: Vec<u8> = PALETTE.iter().rev().flatten().copied().collect();
            let frame = gif::Frame {
                left,
                top,
                width,
                height,
                delay: p.delay,
                transparent: p.trans,
                dispose: p.dispose,
                interlaced: p.interlaced,
                palette: p.local.then_some(reversed),
                buffer: (0..width * height).map(index).collect::<Vec<u8>>().into(),
                ..Default::default()
            };
            enc.write_frame(&frame).unwrap();
        }
    }
    out[11] = bg;
    out
}

/// A GIF whose `frames` 1x1 images, 5/100 s each, each composite onto a `w`x`h` canvas.
fn gif_bomb(w: u16, h: u16, frames: usize) -> Vec<u8> {
    let mut g = b"GIF89a".to_vec();
    g.extend(w.to_le_bytes());
    g.extend(h.to_le_bytes());
    g.extend([0x80, 0, 0, 0, 0, 0, 255, 255, 255]);
    for _ in 0..frames {
        g.extend([0x21, 0xF9, 4, 0, 5, 0, 0, 0]);
        g.extend([0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 2, 0x44, 1, 0]);
    }
    g.push(0x3B);
    g
}

/// A GIF of four 1x1 images on a 3x2 screen where only the third has a Graphic Control
/// Extension (30/100 s, transparent index 1): the others take what the decoder carries over.
fn gif_bare() -> Vec<u8> {
    let mut g = b"GIF89a".to_vec();
    g.extend([3, 0, 2, 0, 0x81, 2, 0]);
    g.extend(PALETTE[..4].as_flattened());
    for (k, at) in [(0u8, 0u8), (1, 1), (1, 2), (3, 1)].into_iter().enumerate() {
        if k == 2 {
            g.extend([0x21, 0xF9, 4, 1, 30, 0, 1, 0]);
        }
        // The LZW codes clear, the index, end.
        let (index, x) = at;
        g.extend([
            0x2C,
            x,
            0,
            0,
            0,
            1,
            0,
            1,
            0,
            0,
            2,
            2,
            0x44 | index << 3,
            1,
            0,
        ]);
    }
    g.push(0x3B);
    g
}

/// A GIF with no global colour table, so no background colour: five 1x1 images on a 4x3
/// screen, each with a table of its own. What no image drew stays as QGIFFormat made the
/// picture, zero bytes, and an image that is cleared when it ends is filled with the
/// picture's first pixel: nothing at first, the third image's colour after it is drawn.
fn gif_nomap() -> Vec<u8> {
    let mut g = b"GIF89a".to_vec();
    g.extend([4, 0, 3, 0, 0, 0, 0]);
    let images = [
        (1u8, 1u8, 0u8, 2u8),
        (2, 0, 1, 2),
        (0, 0, 1, 1),
        (2, 2, 0, 2),
        (3, 0, 1, 0),
    ];
    for (x, y, index, dispose) in images {
        g.extend([0x21, 0xF9, 4, dispose << 2, 5, 0, 0, 0]);
        g.extend([0x2C, x, 0, y, 0, 1, 0, 1, 0, 0x80]);
        g.extend([10, 200, 90, 200, 40, 10]);
        g.extend([2, 2, 0x44 | index << 3, 1, 0]);
    }
    g.push(0x3B);
    g
}

/// One frame of an animated WebP: its rectangle (even offsets), the colour of its top left
/// pixel, its duration, whether it blends over the canvas and whether its rectangle is
/// cleared before the next frame.
#[derive(Clone, Copy)]
struct Anmf {
    rect: [u32; 4],
    rgba: [u8; 4],
    ms: u32,
    blend: bool,
    dispose: bool,
}

/// An animated WebP: the container around the VP8L chunk `image` encodes for each frame.
fn webp(w: u32, h: u32, alpha: bool, background: [u8; 4], frames: &[Anmf]) -> Vec<u8> {
    let u24 = |v: u32| v.to_le_bytes()[..3].to_vec();
    let chunk = |tag: &[u8; 4], body: &[u8]| {
        let mut c = tag.to_vec();
        c.extend((body.len() as u32).to_le_bytes());
        c.extend(body);
        c.resize(c.len() + body.len() % 2, 0);
        c
    };
    let mut body = b"WEBP".to_vec();
    // VP8X: the animation flag and the alpha flag, canvas size minus one.
    let flags = 0x02 | if alpha { 0x10 } else { 0 };
    body.extend(chunk(
        b"VP8X",
        &[vec![flags, 0, 0, 0], u24(w - 1), u24(h - 1)].concat(),
    ));
    // ANIM: background colour (BGRA), loop count 0 (forever).
    let [r, g, b, a] = background;
    body.extend(chunk(b"ANIM", &[b, g, r, a, 0, 0]));
    for f in frames {
        let [x, y, fw, fh] = f.rect;
        let [r, g, b, a] = f.rgba;
        let px = image::RgbaImage::from_fn(fw, fh, |x, y| {
            image::Rgba([r ^ (x as u8 * 16), g ^ (y as u8 * 16), b, a])
        });
        let mut still = Vec::new();
        image::codecs::webp::WebPEncoder::new_lossless(&mut still)
            .encode(&px, fw, fh, image::ExtendedColorType::Rgba8)
            .unwrap();
        assert_eq!(&still[12..16], b"VP8L");
        let bits = u8::from(f.dispose) | if f.blend { 0 } else { 2 };
        let head = [
            u24(x / 2),
            u24(y / 2),
            u24(fw - 1),
            u24(fh - 1),
            u24(f.ms),
            vec![bits],
        ]
        .concat();
        body.extend(chunk(b"ANMF", &[&head[..], &still[12..]].concat()));
    }
    let mut out = b"RIFF".to_vec();
    out.extend((body.len() as u32).to_le_bytes());
    out.extend(body);
    out
}

/// The files both tests play: name, bytes.
fn files() -> Vec<(&'static str, Vec<u8>)> {
    use gif::DisposalMethod::{Background, Keep, Previous};
    let full =
        |w, h, n: u16| -> Vec<Part> { (0..n).map(|k| part([0, 0, w, h], 6000 + k)).collect() };
    // Delays of 0 and 1/100 s play at 100 ms.
    let mut steps = full(6, 4, 6);
    (steps[1].delay, steps[2].delay, steps[3].delay) = (0, 1, 2);
    // Every disposal, with and without a transparent index, on shifted images; one reaches
    // past the screen, one has its own colour table, one is interlaced.
    let on = |rect, trans, dispose| Part {
        trans,
        dispose,
        ..part(rect, 6000)
    };
    let dispose = vec![
        on([0, 0, 8, 8], Some(3), Keep),
        on([2, 2, 3, 3], Some(1), Background),
        on([4, 1, 4, 5], None, Previous),
        on([0, 0, 2, 2], Some(4), Keep),
        on([6, 6, 4, 4], Some(5), Previous),
        on([1, 3, 5, 4], None, Background),
        Part {
            local: true,
            ..on([0, 5, 8, 3], Some(2), Keep)
        },
        Part {
            interlaced: true,
            ..on([0, 0, 8, 8], Some(0), Keep)
        },
    ];
    // The first image has no transparent index and is smaller than the screen: the canvas has
    // no alpha, and the rest of it is the background colour.
    let opaque = vec![
        on([1, 1, 4, 3], None, Background),
        on([0, 0, 3, 3], Some(1), Background),
        on([3, 2, 3, 3], Some(2), Previous),
        on([2, 0, 4, 2], None, Keep),
    ];
    // The first image is transparent and smaller than the screen.
    let clear = vec![
        on([2, 1, 3, 3], Some(0), Keep),
        on([0, 0, 4, 2], Some(1), Background),
        on([1, 1, 5, 4], Some(2), Keep),
    ];
    let frame = |rect, rgba, blend, dispose| Anmf {
        rect,
        rgba,
        ms: 60000,
        blend,
        dispose,
    };
    let blend = [
        frame([0, 0, 8, 6], [200, 40, 10, 255], false, false),
        frame([2, 2, 4, 4], [10, 200, 90, 128], true, true),
        frame([4, 0, 4, 6], [20, 30, 220, 64], true, false),
        frame([0, 2, 6, 4], [90, 90, 90, 0], false, false),
        frame([2, 0, 6, 6], [250, 250, 20, 200], true, false),
    ];
    let solid = [
        Anmf {
            ms: 10,
            ..frame([0, 0, 4, 6], [200, 40, 10, 255], false, false)
        },
        frame([2, 2, 4, 2], [10, 200, 90, 255], true, true),
        frame([0, 4, 2, 2], [20, 30, 220, 255], false, false),
    ];
    vec![
        ("steps.gif", gif(6, 4, 0, &steps)),
        ("dispose.gif", gif(8, 8, 6, &dispose)),
        ("opaque.gif", gif(6, 5, 5, &opaque)),
        ("clear.gif", gif(6, 5, 3, &clear)),
        ("bare.gif", gif_bare()),
        ("nomap.gif", gif_nomap()),
        ("big.gif", gif_bomb(4096, 4096, 8)),
        ("work.gif", gif_bomb(4000, 4000, 40)),
        ("blend.webp", webp(8, 6, true, [0; 4], &blend)),
        ("solid.webp", webp(6, 6, false, [30, 60, 90, 128], &solid)),
    ]
}

fn write_files(dir: &Path) {
    for (name, data) in files() {
        std::fs::write(dir.join(name), data).unwrap();
    }
}

/// Frame count, delays, sizes and pixels of every frame, at the file's size and at the sizes
/// `sourceSize` asks for, are QImageReader's.
#[test]
fn frames_are_qt_image_readers() {
    let dir = tempfile::tempdir().unwrap();
    write_files(dir.path());
    let cases: Vec<(&str, (i32, i32))> = vec![
        ("steps.gif", (0, 0)),
        ("steps.gif", (3, 3)),
        ("steps.gif", (12, 0)),
        ("steps.gif", (0, 2)),
        ("steps.gif", (12, 12)),
        ("dispose.gif", (0, 0)),
        ("dispose.gif", (5, 3)),
        // Under a half on neither side: QImage::scaled paints these.
        ("dispose.gif", (6, 5)),
        ("dispose.gif", (0, 11)),
        ("opaque.gif", (0, 0)),
        ("opaque.gif", (4, 0)),
        // Larger on both sides, on one, on neither.
        ("opaque.gif", (12, 10)),
        ("opaque.gif", (9, 3)),
        ("opaque.gif", (4, 8)),
        ("opaque.gif", (5, 4)),
        ("nomap.gif", (0, 0)),
        ("nomap.gif", (8, 6)),
        ("nomap.gif", (3, 2)),
        ("clear.gif", (0, 0)),
        ("clear.gif", (5, 0)),
        ("clear.gif", (9, 4)),
        ("bare.gif", (0, 0)),
        ("big.gif", (0, 0)),
        ("big.gif", (64, 64)),
        ("work.gif", (0, 0)),
        ("work.gif", (64, 64)),
        ("blend.webp", (0, 0)),
        ("blend.webp", (3, 0)),
        ("blend.webp", (6, 0)),
        ("blend.webp", (5, 4)),
        ("blend.webp", (12, 9)),
        ("solid.webp", (0, 0)),
        ("solid.webp", (2, 2)),
        ("solid.webp", (4, 5)),
        ("solid.webp", (0, 9)),
    ];
    let path = |name: &str| dir.path().join(name).to_str().unwrap().to_string();
    let args: Vec<String> = cases
        .iter()
        .map(|(name, ss)| format!("{} {} {}", path(name), ss.0, ss.1))
        .collect();
    let Some(lines) = qt(READER, Asked::NoWindow, &args) else {
        return;
    };
    for ((name, ss), line) in cases.iter().zip(lines) {
        let what = format!("{name} at {ss:?}");
        let mut want = line.split(' ');
        let count: usize = want.next().unwrap().parse().unwrap();
        let want: Vec<Vec<&str>> = want.map(|f| f.split(':').collect()).collect();
        assert_eq!(want.len(), count, "{what}: Qt read every frame it counted");
        let mut movie = Movie::open(&path(name), *ss).unwrap_or_else(|| panic!("{what} opens"));
        assert_eq!(movie.frame_count(), count, "{what}: frame count");
        let delays: Vec<f64> = want.iter().map(|f| f[0].parse().unwrap()).collect();
        assert_eq!(movie.delays(), delays, "{what}: delays");
        // Scaling a 4000x4000 canvas takes an unoptimized build two seconds: of the scaled
        // frames of the large files three are compared, of every other file all.
        let large = *ss != (0, 0) && matches!(*name, "big.gif" | "work.gif");
        let shown = |k: usize| !large || [0, count / 2, count - 1].contains(&k);
        for (k, f) in want.iter().enumerate().filter(|(k, _)| shown(*k)) {
            let c = movie
                .frame(k)
                .unwrap_or_else(|| panic!("{what}: frame {k}"));
            let size = (f[1].parse().unwrap(), f[2].parse().unwrap());
            assert_eq!((c.w, c.h), size, "{what}: size of frame {k}");
            if f[4] != "-" {
                assert_eq!(hex(&c.px), f[4], "{what}: pixels of frame {k}");
            }
            let sum: u32 = f[3].parse().unwrap();
            assert_eq!(crc(&c.px), sum, "{what}: pixels of frame {k}");
            // What Qt Quick draws without blending, and where that shows.
            let raw = f[5] == "1" && c.px.iter().any(|p| p[3] != 255);
            assert_eq!(
                movie.raw(),
                raw,
                "{what}: frame {k} has no alpha channel, yet alpha"
            );
        }
        // A frame before the one shown is read again from the start of the file.
        for k in [count - 1, 0, count / 2, 1] {
            if large {
                break;
            }
            let c = movie
                .frame(k)
                .unwrap_or_else(|| panic!("{what}: frame {k}"));
            let sum: u32 = want[k][3].parse().unwrap();
            assert_eq!(crc(&c.px), sum, "{what}: pixels of frame {k}, read again");
        }
    }
}

/// A lossy WebP of one picture, as Qt writes it (`QImage::save`, quality 80): 4x2, green.
const STILL_WEBP: &str = "524946463e000000574542505650382032000000d001009d012a0400020001402625\
    a00274ba01f80003b000fee9221ffbcf9fb9f3f73e7fd19fff94fdf238fe471ffca04000";

/// Pictures that do not move, into `dir`: an opaque PNG, a PNG with alpha, a lossy WebP and
/// a lossless one with alpha.
fn write_stills(dir: &Path) {
    let shade = |x: u32, y: u32, a: u8| image::Rgba([(x * 29) as u8, (y * 53) as u8, 200, a]);
    image::RgbaImage::from_fn(9, 6, |x, y| shade(x, y, 255))
        .save(dir.join("still.png"))
        .unwrap();
    let clear = image::RgbaImage::from_fn(7, 5, |x, y| shade(x, y, (x * 40 + y * 3) as u8));
    clear.save(dir.join("clear.png")).unwrap();
    let webp: Vec<u8> = STILL_WEBP
        .as_bytes()
        .chunks(2)
        .map(|h| u8::from_str_radix(std::str::from_utf8(h).unwrap(), 16).unwrap())
        .collect();
    std::fs::write(dir.join("still.webp"), webp).unwrap();
    let mut lossless = Vec::new();
    image::codecs::webp::WebPEncoder::new_lossless(&mut lossless)
        .encode(&clear, 7, 5, image::ExtendedColorType::Rgba8)
        .unwrap();
    std::fs::write(dir.join("lossless.webp"), lossless).unwrap();
}

/// The one frame of a picture that does not move is the picture QImageReader reads, at the
/// file's size and at the sizes `sourceSize` asks for.
#[test]
fn a_still_is_qt_image_readers_picture() {
    let dir = tempfile::tempdir().unwrap();
    write_stills(dir.path());
    let mut cases = Vec::new();
    for name in ["still.png", "clear.png", "still.webp", "lossless.webp"] {
        for ss in [(0, 0), (3, 2), (5, 0), (0, 4), (16, 11), (8, 3)] {
            cases.push((name, ss));
        }
    }
    let path = |name: &str| dir.path().join(name).to_str().unwrap().to_string();
    let args: Vec<String> = cases
        .iter()
        .map(|(name, ss)| format!("{} {} {}", path(name), ss.0, ss.1))
        .collect();
    let Some(lines) = qt(READER, Asked::NoWindow, &args) else {
        return;
    };
    let mut wrong = Vec::new();
    for ((name, ss), line) in cases.iter().zip(lines) {
        let want: Vec<&str> = line.split([' ', ':']).collect();
        let mut movie = Movie::open(&path(name), *ss).expect("the still opens");
        let c = movie.frame(0).expect("its frame");
        let ours = format!("1 {} {} {}", c.w, c.h, hex(&c.px));
        let qt = format!("{} {} {} {}", want[0], want[2], want[3], want[5]);
        if (ours != qt) || movie.frame_count() != 1 {
            wrong.push(format!(
                "{name} at {ss:?}\n   qt:    {qt}\n   shell: {ours}"
            ));
        }
        // Asked for again, the picture is read again.
        assert_eq!(
            hex(&movie.frame(0).expect("its frame again").px),
            hex(&c.px)
        );
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// What needs the QML reader: the scenes here are QML source, which the shipped build does
/// not load.
#[cfg(feature = "interp")]
mod played {
    use super::*;
    use qml::value::Value;
    use qsg::raster::Canvas;
    use shell::scene::{self, Target};

    /// Per `qml-file width height steps` argument shows the scene, sets `currentFrame` to each
    /// of the comma-separated steps and prints `currentFrame:playing:hex` of the window after
    /// each, the first for the scene as loaded. The window is drawn through OpenGL. It
    /// asks for one size only, so the compositor does not tile it; what the compositor gives
    /// it beyond the scene is cut off. No event is handled between a step and its picture: a
    /// movie that a step left playing would show its next frame once its timer was handled.
    const QUICK: &str = r#"
import sys
from PySide6.QtCore import QSize, QUrl
from PySide6.QtGui import QGuiApplication, QImage
from PySide6.QtQuick import QQuickView, QSGRendererInterface
app = QGuiApplication(sys.argv[:1])
for arg in sys.argv[1:]:
    qml, w, h, steps = arg.rsplit(" ", 3)
    v = QQuickView()
    v.setSource(QUrl.fromLocalFile(qml))
    assert v.status() == QQuickView.Status.Ready, v.errors()
    v.setMinimumSize(QSize(int(w), int(h)))
    v.setMaximumSize(QSize(int(w), int(h)))
    v.show()
    while not v.isExposed():
        app.processEvents()
    api = v.rendererInterface().graphicsApi()
    assert api == QSGRendererInterface.GraphicsApi.OpenGL, api
    a = v.rootObject().findChild(object, "a")
    out = []
    for k in [None] + [int(s) for s in steps.split(",") if s]:
        if k is not None:
            a.setProperty("currentFrame", k)
        else:
            app.processEvents()
        px = v.grabWindow()
        assert px.devicePixelRatio() == 1, px.devicePixelRatio()
        assert px.width() >= int(w) and px.height() >= int(h), px.size()
        px = px.copy(0, 0, int(w), int(h)).convertToFormat(QImage.Format_RGBA8888_Premultiplied)
        # An Image has neither property: 0 for both.
        out.append("%d:%d:%s" % (a.property("currentFrame") or 0, a.property("playing") or 0, bytes(px.constBits()).hex()))
    print(" ".join(out))
    v.close()
"#;

    const W: usize = 16;
    const H: usize = 12;

    /// The shell's window for `body` after each step, as the processor draws it and as the
    /// graphics card does: `currentFrame:playing:hex` of each, as QUICK prints.
    fn shell_steps(g: &mut shell::gpu::Gpu, body: &str, steps: &[i32]) -> Vec<[String; 2]> {
        let qml = format!(
            "import QtQuick\nimport Quickshell\nFloatingWindow {{\n    width: {W}; height: {H}\n    \
         color: \"#204060\"\n    property Item img: a\n    {body}\n}}\n"
        );
        let mut rt = qml::runtime::Runtime::new(None);
        let win = rt.load_str(&qml).expect("scene loads");
        let mut fonts = scene::Fonts::default();
        // One surface for all the steps: a step redraws what it changed, as a window does.
        let mut surf = g.pbuffer(W as i32, H as i32).expect("pbuffer");
        surf.reswap();
        let mut out = Vec::new();
        for k in std::iter::once(None).chain(steps.iter().copied().map(Some)) {
            let Value::Item(a) = rt.get(win, "img") else {
                panic!("img");
            };
            if let Some(k) = k {
                rt.set(a, "currentFrame", Value::Num(f64::from(k)));
            }
            for _ in 0..3 {
                rt.settle();
                scene::layout_window(&mut rt, &mut fonts, win, [W as f64, H as f64]);
            }
            let mut c = Canvas::new(W, H, [0, 0, 0, 0]);
            scene::paint_window(&mut rt, &mut fonts, win, &mut Target::Cpu(&mut c));
            let mut fr = shell::gpu::Frame::scaled(W, H, 1.0);
            scene::paint_window(&mut rt, &mut fonts, win, &mut Target::Gpu(&mut fr));
            g.render(&mut surf, &mut fr, || {});
            let frame = rt.get(a, "currentFrame").to_number();
            let state = format!(
                "{}:{}",
                if frame.is_nan() { 0.0 } else { frame },
                u8::from(rt.get(a, "playing").truthy()),
            );
            out.push([c.px, g.read_surface(&surf).px].map(|px| format!("{state}:{}", hex(&px))));
        }
        out
    }

    /// `currentFrame` set by hand: the next frame, a frame further on, a step back, the first
    /// frame, a frame past the end and a negative one show in the shell what they show in Qt
    /// Quick, with `cache` on (Qt keeps the frames it has shown, the shell reads the file again)
    /// and off (QMovie reads only the next frame or the first one), at `sourceSize` too, and
    /// paused. The window is the scene's coloured rectangle with the picture on it, read with
    /// its alpha, from Qt Quick drawing through OpenGL and from the shell drawing on the
    /// processor and on the graphics card.
    ///
    /// A GIF whose first image has no transparent colour is a picture with no alpha channel
    /// whose cleared pixels still have an alpha of 0 (`opaque.gif`, `nomap.gif`). Qt Quick
    /// draws it with blending off, so the window gets those pixels as they are; under an
    /// `opacity` it blends them, and they add to what is under them. A text after `|` in a
    /// case is an item drawn over the picture; `Image: ` before it makes the item an Image,
    /// which shows the first frame.
    #[test]
    fn current_frame_set_by_hand_shows_what_qt_shows() {
        let dir = tempfile::tempdir().unwrap();
        write_files(dir.path());
        // 576 different bytes for an opacity to halve and a stretch to mix.
        image::RgbImage::from_fn(W as u32, H as u32, |x, y| {
            let i = y * W as u32 + x;
            image::Rgb([i as u8, (255 - i) as u8, (i * 7 + 3) as u8])
        })
        .save(dir.path().join("bytes.png"))
        .unwrap();
        let path = |name: &str| dir.path().join(name).to_str().unwrap().to_string();
        let jumps = vec![1, 2, 4, 3, 0, 5, 9, 2, -1, 1, 0];
        let cases: Vec<(&str, &str, Vec<i32>)> = vec![
            ("steps.gif", "", jumps.clone()),
            ("steps.gif", "cache: false", jumps.clone()),
            (
                "steps.gif",
                "cache: false",
                vec![1, 2, 0, 1, 2, 3, 4, 5, 6, 1],
            ),
            ("steps.gif", "sourceSize: Qt.size(3, 3)", jumps.clone()),
            ("steps.gif", "sourceSize.width: 12", vec![3, 1]),
            (
                "steps.gif",
                "sourceSize.height: 2; cache: false",
                vec![1, 0, 3],
            ),
            ("dispose.gif", "", vec![1, 2, 3, 4, 5, 6, 7, 8, 5, 2, 6, 3]),
            (
                "dispose.gif",
                "cache: false",
                vec![1, 2, 3, 4, 5, 6, 7, 8, 1],
            ),
            (
                "dispose.gif",
                "x: 3; y: 2; sourceSize: Qt.size(5, 3)",
                vec![2, 1, 7],
            ),
            ("opaque.gif", "", vec![1, 2, 3, 1, 0, 2]),
            ("opaque.gif", "opacity: 0.5", vec![1, 2, 3]),
            // Blending is off above an opacity of 0.999.
            ("opaque.gif", "opacity: 0.999", vec![2]),
            ("opaque.gif", "opacity: 0.9995", vec![2]),
            ("opaque.gif", "layer.enabled: true", vec![2, 3]),
            ("opaque.gif", "layer.enabled: true; opacity: 0.5", vec![2]),
            ("opaque.gif", "x: 1; y: 1; width: 12; height: 10", vec![2]),
            (
                "opaque.gif",
                "x: 1; width: 12; height: 10; smooth: true",
                vec![2],
            ),
            ("opaque.gif", "sourceSize: Qt.size(12, 10)", vec![2, 3]),
            (
                "opaque.gif",
                "sourceSize: Qt.size(12, 10); opacity: 0.5",
                vec![2],
            ),
            ("opaque.gif", "sourceSize.width: 4", vec![2]),
            (
                "opaque.gif",
                "| Rectangle { x: 2; y: 1; width: 3; height: 3; color: \"#80ff0000\" }",
                vec![2, 3],
            ),
            (
                "opaque.gif",
                "| Rectangle { width: 4; height: 2; opacity: 0.25; color: \"black\" }",
                vec![2],
            ),
            ("nomap.gif", "", vec![1, 2, 3, 4, 1]),
            ("nomap.gif", "opacity: 0.5", vec![1, 3]),
            ("nomap.gif", "x: 2; y: 1; width: 8; height: 6", vec![3, 4]),
            ("nomap.gif", "layer.enabled: true", vec![3]),
            // The first frame in an Image: the same picture, drawn the same way.
            ("opaque.gif", "Image: ", vec![]),
            ("opaque.gif", "Image: opacity: 0.5", vec![]),
            ("nomap.gif", "Image: ", vec![]),
            ("nomap.gif", "Image: opacity: 0.5", vec![]),
            (
                "nomap.gif",
                "Image: x: 2; y: 1; width: 8; height: 6",
                vec![],
            ),
            // Every half of a byte times an opacity, and the bytes between two texels, round
            // as the graphics card rounds them.
            ("bytes.png", "Image: opacity: 0.5", vec![]),
            ("bytes.png", "Image: opacity: 0.3", vec![]),
            ("bytes.png", "Image: opacity: 0.7", vec![]),
            (
                "bytes.png",
                "Image: x: 1; y: 1; width: 11; height: 7; smooth: true",
                vec![],
            ),
            (
                "bytes.png",
                "Image: sourceSize: Qt.size(5, 4); width: 16; height: 12; smooth: true; opacity: 0.5",
                vec![],
            ),
            ("clear.gif", "x: 5", vec![1, 2, 1, 0]),
            ("bare.gif", "", vec![1, 2, 3, 2]),
            // No NETSCAPE extension: the file asks for one round, the frame past the last one
            // ends the movie, and Qt Quick starts it again.
            ("bare.gif", "", vec![1, 3, 4]),
            ("bare.gif", "cache: false", vec![1, 2, 3, 4]),
            // A paused movie keeps its frame when it cannot give the one asked for.
            ("steps.gif", "paused: true", vec![2, 9, 1, 6, 3]),
            // A movie that is not playing is not paused either.
            ("steps.gif", "paused: true; playing: false", vec![2, 9, 1]),
            (
                "steps.gif",
                "paused: true; cache: false",
                vec![1, 2, 4, 3, 0, 1],
            ),
            ("blend.webp", "", vec![1, 2, 3, 4, 2, 1, 0, 3]),
            ("blend.webp", "cache: false", vec![1, 2, 3, 4, 5, 1, 3]),
            ("blend.webp", "sourceSize.width: 3", vec![2, 1]),
            ("solid.webp", "", vec![1, 2, 1, 7, 2]),
        ];
        let body = |name: &str, props: &str| {
            let playing = match props.contains("paused") {
                true => "",
                false => "playing: false;",
            };
            let smooth = match props.contains("smooth") {
                true => "",
                false => "smooth: false;",
            };
            let (props, over) = props.split_once('|').unwrap_or((props, ""));
            let (kind, playing, props) = match props.strip_prefix("Image: ") {
                Some(props) => ("Image", "", props),
                None => ("AnimatedImage", playing, props),
            };
            format!(
                "{kind} {{ id: a; objectName: \"a\"; {playing} {smooth} \
             source: \"{}\"; {props} }}\n{over}",
                path(name)
            )
        };
        let list = |steps: &[i32]| steps.iter().map(i32::to_string).collect::<Vec<_>>();
        let args: Vec<String> = cases
        .iter()
        .enumerate()
        .map(|(n, (name, props, steps))| {
            let qml = dir.path().join(format!("{n}.qml"));
            let scene = format!(
                "import QtQuick\nRectangle {{\n    width: {W}; height: {H}; color: \"#204060\"\n    {}\n}}\n",
                body(name, props)
            );
            std::fs::write(&qml, scene).unwrap();
            format!("{} {W} {H} {}", qml.display(), list(steps).join(","))
        })
        .collect();
        let Some(lines) = qt(QUICK, Asked::Window, &args) else {
            return;
        };
        let (mut g, _) =
            shell::gpu::Gpu::headless("surfaceless").expect("surfaceless EGL/GLES2 context (Mesa)");
        for ((name, props, steps), line) in cases.iter().zip(lines) {
            let ours = shell_steps(&mut g, &body(name, props), steps);
            let asked = std::iter::once("load".to_string()).chain(list(steps));
            for ((ours, qt), k) in ours.iter().zip(line.split(' ')).zip(asked) {
                let qt: Vec<&str> = qt.split(':').collect();
                for (ours, by) in ours.iter().zip(["the processor", "the graphics card"]) {
                    let ours: Vec<&str> = ours.split(':').collect();
                    let what = format!("{name} {{ {props} }} steps {steps:?}, after {k}, {by}");
                    assert_eq!(ours[..2], qt[..2], "{what}: currentFrame, playing");
                    assert_eq!(ours[2], qt[2], "{what}: the window");
                }
            }
        }
    }

    /// Each window plays until its movie stopped or logged `play` signals; a stopped one then
    /// gets `currentFrame` set by hand and `playing = true`. A movie's timer is an event, so the
    /// loop waits for events, not for time.
    const ROUNDS: &str = r#"
import sys
from PySide6.QtCore import QUrl, QEventLoop
from PySide6.QtGui import QGuiApplication
from PySide6.QtQuick import QQuickView
app = QGuiApplication(sys.argv[:1])
play, again = int(sys.argv[1]), int(sys.argv[2])
steps = [int(s) for s in sys.argv[3].split(",")]
views = []
for qml in sys.argv[4:]:
    v = QQuickView()
    v.setSource(QUrl.fromLocalFile(qml))
    assert v.status() == QQuickView.Status.Ready, v.errors()
    v.show()
    views.append((v, v.rootObject(), v.rootObject().findChild(object, "a")))
def log(root):
    return root.property("log").split()
def state(a):
    return "%d:%d:%d" % (a.property("currentFrame"), a.property("playing"), a.property("frameCount"))
def run(which, limit):
    while any(a.property("playing") and len(log(root)) < limit for _, root, a in which):
        app.processEvents(QEventLoop.ProcessEventsFlag.WaitForMoreEvents)
    return [",".join(log(root)[:limit]) or "-" for _, root, a in which]
lines = [[text] for text in run(views, play)]
stopped = []
for (v, root, a), line in zip(views, lines):
    if a.property("playing"):
        line.append("plays")
        continue
    line.append(state(a))
    root.setProperty("log", "")
    for k in steps:
        a.setProperty("currentFrame", k)
        line.append(state(a))
    line.append(",".join(log(root)) or "-")
    root.setProperty("log", "")
    a.setProperty("playing", True)
    stopped.append(((v, root, a), line))
for line, text in zip([line for _, line in stopped], run([view for view, _ in stopped], again)):
    line.append(text)
for line in lines:
    print(" ".join(line))
"#;

    const PLAY: usize = 26;
    const AGAIN: usize = 12;
    const BY_HAND: [i32; 6] = [1, 2, 0, 5, 2, 1];

    /// `frames` frames of 30 ms; `netscape` is the loop count of the NETSCAPE2.0 block, if any.
    fn gif_rounds(frames: u8, netscape: Option<u16>) -> Vec<u8> {
        let mut g = b"GIF89a".to_vec();
        g.extend([4, 0, 2, 0, 0x81, 0, 0]);
        g.extend(PALETTE[..4].as_flattened());
        if let Some(n) = netscape {
            g.extend([0x21, 0xFF, 11]);
            g.extend(b"NETSCAPE2.0");
            g.extend([3, 1]);
            g.extend(n.to_le_bytes());
            g.push(0);
        }
        for k in 0..frames {
            g.extend([0x21, 0xF9, 4, 0, 3, 0, 0, 0]);
            g.extend([0x2C, k, 0, 0, 0, 1, 0, 1, 0, 0, 2, 2, 0x44 | k << 3, 1, 0]);
        }
        g.push(0x3B);
        g
    }

    /// `frames` frames of 30 ms with `loops` in the ANIM chunk.
    fn webp_rounds(frames: usize, loops: u8) -> Vec<u8> {
        let frame = |x: u32, rgba| Anmf {
            rect: [x, 0, 2, 2],
            rgba,
            ms: 30,
            blend: false,
            dispose: false,
        };
        let all = [
            frame(0, [255, 0, 0, 255]),
            frame(2, [0, 255, 0, 255]),
            frame(4, [0, 0, 255, 255]),
        ];
        let mut b = webp(6, 2, true, [0, 0, 0, 0], &all[..frames]);
        // ANIM follows VP8X (12 + 8 + 10 bytes): tag, size, background, loop count.
        assert_eq!(&b[30..34], b"ANIM");
        b[42] = loops;
        b
    }

    /// The AnimatedImage of a rounds case, logging its signals in the order they come.
    fn rounds_item(file: &str, props: &str) -> String {
        format!(
            "AnimatedImage {{ id: a; objectName: \"a\"; smooth: false; source: \"{file}\"; {props}\n \
         onCurrentFrameChanged: root.log += \"f\" + currentFrame + \" \"\n \
         onPlayingChanged: root.log += \"p\" + (playing ? 1 : 0) + \" \"\n \
         onFinished: root.log += \"F \" }}"
        )
    }

    /// `rounds_item` whose handlers say `frameCount` as they read it too, and when it changed.
    fn counting_item(file: &str, props: &str) -> String {
        format!(
            "AnimatedImage {{ id: a; objectName: \"a\"; smooth: false; source: \"{file}\"; {props}\n \
         onFrameCountChanged: root.log += \"n\" + frameCount + \" \"\n \
         onCurrentFrameChanged: root.log += \"f\" + currentFrame + \"/\" + frameCount + \" \"\n \
         onPlayingChanged: root.log += \"p\" + (playing ? 1 : 0) + \"/\" + frameCount + \" \"\n \
         onFinished: root.log += \"F \" }}"
        )
    }

    /// What ROUNDS prints for each of `items` shown by Qt Quick, a line an item: the same with
    /// its windows drawn through OpenGL and not drawn at all.
    fn qt_rounds(dir: &Path, name: &str, items: &[String]) -> Vec<String> {
        let steps: Vec<String> = BY_HAND.iter().map(|k| k.to_string()).collect();
        let mut args = vec![PLAY.to_string(), AGAIN.to_string(), steps.join(",")];
        for (n, item) in items.iter().enumerate() {
            let qml = dir.join(format!("{name}.{n}.qml"));
            let scene = format!(
                "import QtQuick\nRectangle {{\n    id: root\n    width: {W}; height: {H}\n    \
                 color: \"#204060\"\n    property string log: \"\"\n    {item}\n}}\n"
            );
            std::fs::write(&qml, scene).unwrap();
            args.push(qml.display().to_string());
        }
        let lines = |asked| {
            let out = python(ROUNDS, asked).args(&args).output().unwrap();
            let text = String::from_utf8_lossy(&out.stdout);
            assert!(
                out.status.success(),
                "{name}: {text}{}",
                String::from_utf8_lossy(&out.stderr)
            );
            let lines: Vec<String> = text.lines().map(str::to_string).collect();
            assert_eq!(lines.len(), items.len(), "{name}: {text}");
            lines
        };
        let (drawn, lines) = (lines(Asked::Window), lines(Asked::NoWindow));
        assert_eq!(drawn, lines, "{name}: through OpenGL and with no window");
        lines
    }

    /// What ROUNDS prints for one window, from the shell: the movie's timer is the runtime's
    /// clock, stepped by the frame delay.
    fn shell_rounds(item: &str) -> String {
        let qml = format!(
            "import QtQuick\nimport Quickshell\nFloatingWindow {{\n    id: root\n    \
         width: {W}; height: {H}\n    color: \"#204060\"\n    property string log: \"\"\n    \
         property Item img: a\n    {item}\n}}\n"
        );
        let mut rt = qml::runtime::Runtime::new(None);
        let win = rt.load_str(&qml).expect("scene loads");
        let mut fonts = scene::Fonts::default();
        let Value::Item(a) = rt.get(win, "img") else {
            panic!("img");
        };
        let mut show = |rt: &mut qml::runtime::Runtime| {
            for _ in 0..3 {
                rt.settle();
                scene::layout_window(rt, &mut fonts, win, [W as f64, H as f64]);
            }
            let mut c = Canvas::new(W, H, [0, 0, 0, 0]);
            scene::paint_window(rt, &mut fonts, win, &mut Target::Cpu(&mut c));
        };
        let log = |rt: &mut qml::runtime::Runtime| -> Vec<String> {
            let text = rt.get(win, "log").to_js_string();
            text.split_whitespace().map(str::to_string).collect()
        };
        let playing = |rt: &mut qml::runtime::Runtime| rt.get(a, "playing").truthy();
        let state = |rt: &mut qml::runtime::Runtime| {
            format!(
                "{}:{}:{}",
                rt.get(a, "currentFrame").to_number(),
                u8::from(rt.get(a, "playing").truthy()),
                rt.get(a, "frameCount").to_number()
            )
        };
        let mut now = 0.0;
        type Show<'a> = &'a mut dyn FnMut(&mut qml::runtime::Runtime);
        let run = |rt: &mut qml::runtime::Runtime, show: Show, now: &mut f64, limit: usize| {
            let mut ticks = 0;
            while playing(rt) && log(rt).len() < limit {
                *now += 30.0;
                rt.tick(*now);
                show(rt);
                ticks += 1;
                assert!(
                    ticks < 4 * limit,
                    "the movie neither plays nor stops: {item}"
                );
            }
            let seen = log(rt);
            let text = seen[..seen.len().min(limit)].join(",");
            if text.is_empty() {
                "-".to_string()
            } else {
                text
            }
        };
        rt.tick(now);
        show(&mut rt);
        let mut line = vec![run(&mut rt, &mut show, &mut now, PLAY)];
        if playing(&mut rt) {
            line.push("plays".to_string());
            return line.join(" ");
        }
        line.push(state(&mut rt));
        rt.set(win, "log", Value::Str("".into()));
        for k in BY_HAND {
            rt.set(a, "currentFrame", Value::Num(f64::from(k)));
            show(&mut rt);
            line.push(state(&mut rt));
        }
        let by_hand = log(&mut rt).join(",");
        line.push(if by_hand.is_empty() {
            "-".to_string()
        } else {
            by_hand
        });
        rt.set(win, "log", Value::Str("".into()));
        rt.set(a, "playing", Value::Bool(true));
        show(&mut rt);
        line.push(run(&mut rt, &mut show, &mut now, AGAIN));
        line.join(" ")
    }

    /// `currentFrameChanged`, `playingChanged` and `finished` come in the order Qt Quick 6.12
    /// emits them: for GIF and WebP files of three frames and of one, which ask for no, one,
    /// three and endless further rounds; with `loops` left alone, 0, 1, 2, 5 and negative, both
    /// values of `finishBehavior`, and `cache` on and off. With `loops` left alone a movie of
    /// several frames never stops: the end of the file's rounds shows as `playing` going false
    /// and true again. A movie that stopped then takes `currentFrame` set by hand and
    /// `playing = true` as Qt's does.
    ///
    /// Qt 6.12 itself dies (endless recursion in handleLoopCompletion) when `loops`, 0 too, ends
    /// a movie with `cache: false` and `FinishAtFinalFrame`: there the shell stops at the last
    /// frame, with the signals of the same movie with `cache` on.
    #[test]
    fn rounds_and_their_signals_are_qts() {
        let dir = tempfile::tempdir().unwrap();
        let path = |name: &str| dir.path().join(name).to_str().unwrap().to_string();
        let files: Vec<(&str, Vec<u8>)> = vec![
            ("none.gif", gif_rounds(3, None)),
            ("n0.gif", gif_rounds(3, Some(0))),
            ("n1.gif", gif_rounds(3, Some(1))),
            ("n3.gif", gif_rounds(3, Some(3))),
            ("one.gif", gif_rounds(1, None)),
            ("one0.gif", gif_rounds(1, Some(0))),
            ("l0.webp", webp_rounds(3, 0)),
            ("l1.webp", webp_rounds(3, 1)),
            ("l3.webp", webp_rounds(3, 3)),
            ("one.webp", webp_rounds(1, 1)),
            ("one0.webp", webp_rounds(1, 0)),
        ];
        // (props, whether Qt survives them, the case with `cache` on).
        let mut props: Vec<(String, bool, usize)> = Vec::new();
        for loops in [None, Some(0), Some(1), Some(2), Some(5), Some(-5)] {
            for last in [false, true] {
                let twin = props.len();
                for cache in [true, false] {
                    let mut p = String::new();
                    if let Some(n) = loops {
                        p += &format!("loops: {n}; ");
                    }
                    if last {
                        p += "finishBehavior: AnimatedImage.FinishAtFinalFrame; ";
                    }
                    if !cache {
                        p += "cache: false; ";
                    }
                    let dies = !cache && last && loops.is_some_and(|n| n >= 0);
                    props.push((p, !dies, twin));
                }
            }
        }
        let per_file = |name: &str| -> Vec<String> {
            let items: Vec<String> = props
                .iter()
                .filter(|(_, alive, _)| *alive)
                .map(|(p, _, _)| rounds_item(&path(name), p))
                .collect();
            qt_rounds(dir.path(), name, &items)
        };
        if !Path::new(PYTHON).exists() {
            testenv::equiv_missing(format_args!("{PYTHON} (PySide6)"));
            return;
        }
        for (name, bytes) in &files {
            std::fs::write(path(name), bytes).unwrap();
        }
        let qt: Vec<Vec<String>> = std::thread::scope(|s| {
            let jobs: Vec<_> = files
                .iter()
                .map(|(name, _)| s.spawn(|| per_file(name)))
                .collect();
            jobs.into_iter().map(|j| j.join().unwrap()).collect()
        });
        let mut wrong = Vec::new();
        let (mut stopped, mut played) = (0, 0);
        for ((name, _), lines) in files.iter().zip(&qt) {
            let mut lines = lines.iter();
            let mut theirs: Vec<Option<&String>> = Vec::new();
            for (_, alive, _) in &props {
                theirs.push(if *alive { lines.next() } else { None });
            }
            for (n, (p, _, twin)) in props.iter().enumerate() {
                let ours = shell_rounds(&rounds_item(&path(name), p));
                let same = match theirs[n] {
                    Some(qt) => {
                        if qt.ends_with(" plays") {
                            played += 1;
                        } else {
                            stopped += 1;
                        }
                        ours == *qt
                    }
                    // The play and the frame it stops at; by hand `cache: false` differs.
                    None => {
                        let head = |l: &str| l.split(' ').take(2).collect::<Vec<_>>().join(" ");
                        head(&ours) == head(theirs[*twin].unwrap())
                    }
                };
                if !same {
                    wrong.push(format!(
                        "{name} {{ {p}}}\n   qt:    {}\n   shell: {ours}",
                        theirs[n].or(theirs[*twin]).unwrap()
                    ));
                }
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
        // Both kinds of case were there: movies that stopped and movies that play on.
        assert_eq!((stopped, played), (130, 90));
    }

    /// A picture that does not move is a movie of one frame to Qt Quick 6.12, and to the shell:
    /// `frameCountChanged`, `currentFrameChanged` and `playingChanged` come in Qt's order with
    /// Qt's values, the movie stops by itself (a PNG after QMovie's 1000 ms for a format that
    /// is no animation, a WebP at once), never says `finished`, and takes `currentFrame` set
    /// by hand and `playing = true` as Qt's does. QMovie does not know how many frames a PNG
    /// has between reading its one frame and looking for the next: `frameCount` reads 0 then,
    /// with no signal, in the handlers too.
    #[test]
    fn stills_are_movies_of_one_frame_as_in_qt() {
        if !Path::new(PYTHON).exists() {
            testenv::equiv_missing(format_args!("{PYTHON} (PySide6)"));
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = |name: &str| dir.path().join(name).to_str().unwrap().to_string();
        write_stills(dir.path());
        let props = [
            "",
            "loops: 1; ",
            "loops: 2; finishBehavior: AnimatedImage.FinishAtFinalFrame; ",
            "loops: 2; finishBehavior: AnimatedImage.FinishAtFinalFrame; cache: false; ",
            "cache: false; ",
            "playing: false; ",
            "loops: 0; ",
            "currentFrame: 1; ",
        ];
        let mut wrong = Vec::new();
        for name in ["still.png", "clear.png", "still.webp", "lossless.webp"] {
            let items: Vec<String> = props
                .iter()
                .map(|p| counting_item(&path(name), p))
                .collect();
            let qt = qt_rounds(dir.path(), name, &items);
            for ((p, item), qt) in props.iter().zip(&items).zip(qt) {
                let ours = shell_rounds(item);
                if ours != qt {
                    wrong.push(format!("{name} {{ {p}}}\n   qt:    {qt}\n   shell: {ours}"));
                }
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    /// A deliberate difference (docs/qt-differences.md): Qt 6.12 dies of a segmentation fault
    /// when `loops` ends a movie of several frames with `cache: false` and
    /// `FinishAtFinalFrame`. The shell stops at the last frame and says `finished`, as Qt does
    /// with `cache` on (`rounds_and_their_signals_are_qts` compares the two). If Qt stops
    /// dying here, this test fails and the case joins that comparison.
    #[test]
    fn where_qt_dies_the_shell_stops_at_the_last_frame() {
        use std::os::unix::process::ExitStatusExt;
        if !Path::new(PYTHON).exists() {
            testenv::equiv_missing(format_args!("{PYTHON} (PySide6)"));
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let steps: Vec<String> = BY_HAND.iter().map(|k| k.to_string()).collect();
        let files = [
            ("n1.gif", gif_rounds(3, Some(1))),
            ("l1.webp", webp_rounds(3, 1)),
        ];
        for (name, bytes) in &files {
            let file = dir.path().join(name).to_str().unwrap().to_string();
            std::fs::write(&file, bytes).unwrap();
            // `loops: 0` plays nothing, with `cache` on in Qt too.
            for (loops, stops) in [(0, "0:0:3"), (1, "2:0:3"), (2, "2:0:3")] {
                let p = format!(
                    "loops: {loops}; finishBehavior: AnimatedImage.FinishAtFinalFrame; \
                     cache: false; "
                );
                let qml = dir.path().join(format!("{name}.{loops}.qml"));
                let scene = format!(
                    "import QtQuick\nRectangle {{\n    id: root\n    width: {W}; height: {H}\n    \
                     property string log: \"\"\n    {}\n}}\n",
                    rounds_item(&file, &p)
                );
                std::fs::write(&qml, scene).unwrap();
                for asked in [Asked::NoWindow, Asked::Window] {
                    let out = python(ROUNDS, asked)
                        .args([PLAY.to_string(), AGAIN.to_string(), steps.join(",")])
                        .arg(&qml)
                        .output()
                        .unwrap();
                    assert_eq!(
                        out.status.signal(),
                        Some(11),
                        "{name} {{ {p}}}: Qt lives: {}",
                        String::from_utf8_lossy(&out.stdout)
                    );
                }
                let ours = shell_rounds(&rounds_item(&file, &p));
                let mut ours = ours.split(' ');
                let (play, at) = (ours.next().unwrap(), ours.next().unwrap());
                let ends = play.ends_with(",p0,F") || play.contains(",p0,f2,F");
                assert_eq!(
                    (at, loops == 0 || ends, play == "f0,p0"),
                    (stops, true, loops == 0),
                    "{name} {{ {p}}}: {play}"
                );
            }
        }
    }
}
