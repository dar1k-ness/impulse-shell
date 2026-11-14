//! PNG and TIFF of 16 bits per sample against Qt itself: QImage keeps them at 16 bits per
//! channel (RGBX64, RGBA64, Grayscale16) and the scene graph converts that to 8-bit
//! premultiplied pixels, rounding each channel and multiplying alpha in before the cut. A TIFF
//! with associated alpha is premultiplied already. Read live through the PySide6 of ri-testenv
//! (Qt 6.12). The files are imgtools' tests/fixtures/convert (its make.py).
//!
//! rgb16_trns.png (16-bit RGB with a `tRNS` colour) is left out: Qt 6.12 reads its three-sample
//! rows as four-sample pixels and shows noise.
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The PySide6 of the testenv image (tools/testenv/Containerfile).
const PYTHON: &str = "/opt/qv4-venv/bin/python";

const FILES: [&str; 10] = [
    "rgb16.png",
    "rgba16.png",
    "gray16.png",
    "graya16.png",
    "rgb16.tiff",
    "rgba16.tiff",
    "gray16.tiff",
    "rgba16_assoc.tiff",
    // 8 bits per sample, with unassociated and with associated alpha.
    "rgba.tiff",
    "rgba_assoc.tiff",
];

fn file(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../imgtools/tests/fixtures/convert")
        .join(name)
}

/// For every file of argv[1..]: width, height and `hasAlphaChannel()` as three little-endian
/// u32, then the image as premultiplied RGBA, the format a texture is uploaded in.
const QT: &str = r#"
import struct, sys
from PySide6.QtGui import QGuiApplication, QImage
app = QGuiApplication(sys.argv[:1])
for f in sys.argv[1:]:
    img = QImage(f)
    assert not img.isNull(), f
    px = img.convertToFormat(QImage.Format.Format_RGBA8888_Premultiplied)
    w, h = px.width(), px.height()
    sys.stdout.buffer.write(struct.pack("<III", w, h, img.hasAlphaChannel()))
    for y in range(h):
        sys.stdout.buffer.write(bytes(px.constScanLine(y))[: w * 4])
"#;

#[test]
fn deep_samples_and_tiff_alpha_become_qts_pixels() {
    if !Path::new(PYTHON).exists() {
        testenv::equiv_missing(format_args!("{PYTHON} (PySide6)"));
        return;
    }
    let out = Command::new(PYTHON)
        .args(["-c", QT])
        .args(FILES.map(file))
        .env("QT_QPA_PLATFORM", "offscreen")
        .stderr(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success(), "PySide6 failed");
    let mut raw = &out.stdout[..];
    let u32le = |raw: &mut &[u8]| {
        let (v, rest) = raw.split_first_chunk::<4>().unwrap();
        *raw = rest;
        u32::from_le_bytes(*v) as usize
    };
    let mut bad = Vec::new();
    for f in FILES {
        let (w, h, alpha) = (u32le(&mut raw), u32le(&mut raw), u32le(&mut raw));
        let (want, rest) = raw.split_at(w * h * 4);
        raw = rest;
        let (got, got_alpha) = qsg::image::load_ex(file(f).to_str().unwrap()).unwrap();
        assert_eq!((got.w, got.h), (w, h), "{f}");
        assert_eq!(got_alpha, alpha != 0, "{f}: alpha");
        let diff = |(a, b): (&u8, &u8)| a.abs_diff(*b);
        let px = got.px.as_flattened();
        let n = px.iter().zip(want).filter(|&p| diff(p) > 0).count();
        let max = px.iter().zip(want).map(diff).max().unwrap_or(0);
        if n > 0 {
            bad.push(format!("{f}: {n} bytes differ from Qt, by up to {max}"));
        }
    }
    assert!(raw.is_empty());
    assert!(bad.is_empty(), "{bad:#?}");
}
