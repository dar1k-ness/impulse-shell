//! CMYK / YCCK JPEG, ICO and GIF against Qt 6.12 itself: QImage loads the files of
//! fixtures/formats (make.py there writes them) and the test compares its size, whether it has
//! an alpha channel and its pixels with `qsg::image::load_ex`.
use std::path::{Path, PathBuf};

/// The PySide6 of the testenv image (tools/testenv/Containerfile).
const PYTHON: &str = "/opt/qv4-venv/bin/python";

/// Prints `name width height alpha(0|1) hex(RGBA8888_Premultiplied bytes)` per file.
const ORACLE: &str = r#"
import sys
from PySide6.QtGui import QImage
for path in sys.argv[1:]:
    im = QImage(path)
    assert not im.isNull(), path
    px = im.convertToFormat(QImage.Format_RGBA8888_Premultiplied)
    rows = b"".join(bytes(px.constScanLine(y))[: 4 * px.width()] for y in range(px.height()))
    print(path, im.width(), im.height(), int(im.hasAlphaChannel()), rows.hex())
"#;

struct QtImage {
    w: usize,
    h: usize,
    alpha: bool,
    px: Vec<[u8; 4]>,
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/formats")
        .join(name)
}

/// Qt's reading of each fixture; `None` (reported) without PySide6.
fn qt(names: &[&str]) -> Option<Vec<QtImage>> {
    if !Path::new(PYTHON).exists() {
        testenv::equiv_missing(format_args!("{PYTHON} (PySide6)"));
        return None;
    }
    let out = std::process::Command::new(PYTHON)
        .args(["-c", ORACLE])
        .args(names.iter().map(|n| fixture(n)))
        .env("QT_QPA_PLATFORM", "offscreen")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let images: Vec<QtImage> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| {
            let f: Vec<&str> = l.rsplitn(5, ' ').collect();
            let px = (0..f[0].len() / 8)
                .map(|i| {
                    let v = u32::from_str_radix(&f[0][8 * i..8 * i + 8], 16).unwrap();
                    v.to_be_bytes()
                })
                .collect();
            QtImage {
                w: f[3].parse().unwrap(),
                h: f[2].parse().unwrap(),
                alpha: f[1] == "1",
                px,
            }
        })
        .collect();
    assert_eq!(images.len(), names.len());
    Some(images)
}

fn assert_same(name: &str, want: &QtImage, c: &qsg::raster::Canvas, alpha: bool) {
    assert_eq!((c.w, c.h), (want.w, want.h), "{name}: size");
    assert_eq!(alpha, want.alpha, "{name}: alpha");
    let differ = c.px.iter().zip(&want.px).filter(|(a, b)| a != b).count();
    assert_eq!(differ, 0, "{name}: pixels that differ from Qt's");
}

fn same_as_qt(names: &[&str]) {
    let Some(want) = qt(names) else { return };
    for (name, want) in names.iter().zip(&want) {
        let (c, alpha) = qsg::image::load_ex(fixture(name).to_str().unwrap())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_same(name, want, &c, alpha);
    }
}

/// QJpegHandler reads four components as Format_CMYK8888 (samples inverted, with or without an
/// Adobe marker); TurboJPEG alone refuses to turn them into RGB.
#[test]
fn cmyk_and_ycck_jpeg_are_drawn_as_qt_draws_them() {
    let names = ["cmyk.jpg", "ycck.jpg", "cmyk_plain.jpg"];
    same_as_qt(&names);
    // The wallpaper path, which decodes at full size when the target is not smaller.
    let Some(want) = qt(&names) else { return };
    for (name, want) in names.iter().zip(&want) {
        let (c, alpha, size) =
            qsg::image::load_scaled(fixture(name).to_str().unwrap(), 4096.0).unwrap();
        assert_eq!(size, (want.w, want.h));
        assert_same(name, want, &c, alpha);
    }
}

/// A CMYK JPEG decoded at a DCT scale (1/2 here) has the colours of the full-size one.
#[test]
fn scaled_cmyk_jpeg_keeps_its_colours() {
    let path = fixture("cmyk.jpg");
    let (full, _) = qsg::image::load_ex(path.to_str().unwrap()).unwrap();
    let (half, alpha, size) = qsg::image::load_scaled(path.to_str().unwrap(), 100.0).unwrap();
    assert_eq!((size, half.w, half.h, alpha), ((200, 150), 100, 75, false));
    // The pink block covers x 30..120, y 20..90 of the full image.
    let (a, b) = (full.px[50 * 200 + 70], half.px[25 * 100 + 35]);
    assert!(
        (0..3).all(|k| a[k].abs_diff(b[k]) <= 2),
        "full {a:?}, half {b:?}"
    );
    assert!(a[0] > 200 && a[1] < 90, "{a:?}");
}

/// QtIcoHandler gives the first entry of the directory (`image` picks the largest) and hands
/// a PNG entry to the PNG reader, whatever its colour type.
#[test]
fn ico_is_the_entry_qt_gives() {
    same_as_qt(&["multi.ico", "png.ico", "graypng.ico"]);
}

/// QGifHandler gives a GIF alpha only when its first frame names a transparent colour.
#[test]
fn gif_alpha_follows_the_transparent_colour() {
    same_as_qt(&["opaque.gif", "transparent.gif", "unused.gif"]);
    // Without Qt: the same expectation, as Qt 6.12 answered it.
    for (name, want) in [
        ("opaque.gif", false),
        ("transparent.gif", true),
        ("unused.gif", true),
    ] {
        let (_, alpha) = qsg::image::load_ex(fixture(name).to_str().unwrap()).unwrap();
        assert_eq!(alpha, want, "{name}");
    }
}
