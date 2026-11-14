//! QImage::scaled(SmoothTransformation) of a Format_ARGB32 image against Qt 6.12 itself, over
//! a grid of sizes, factors and kinds of alpha: what an Image or AnimatedImage with a
//! `sourceSize` shows of a PNG or WebP with an alpha channel. Read live through the PySide6
//! of ri-testenv.
//!
//! This is the scaling Qt does on the processor when it reads a file (QImageReader with a
//! scaled size), and the answer is the QImage's own bits: nothing is drawn, so the platform
//! (`offscreen`) has no part in it. Stretching a picture to the size of its item, `smooth`
//! and `mipmap` are the graphics card's and are not compared here.
use std::fmt::Write as _;

/// The PySide6 of the testenv image (tools/testenv/Containerfile).
const PYTHON: &str = "/opt/qv4-venv/bin/python";

/// Per line `w h dw dh hex` of the file named (the hex being RGBA bytes, not premultiplied)
/// prints the hex of the image scaled to `dw` x `dh`, as RGBA8888_Premultiplied.
const SCALED: &str = r#"
import sys
from PySide6.QtCore import Qt
from PySide6.QtGui import QGuiApplication, QImage
app = QGuiApplication(sys.argv[:1])
for line in open(sys.argv[1]):
    w, h, dw, dh, px = line.split()
    w, h, dw, dh = int(w), int(h), int(dw), int(dh)
    raw = bytes.fromhex(px)
    im = QImage(raw, w, h, w * 4, QImage.Format_RGBA8888).convertToFormat(QImage.Format_ARGB32)
    out = im.scaled(dw, dh, Qt.IgnoreAspectRatio, Qt.SmoothTransformation)
    out = out.convertToFormat(QImage.Format_RGBA8888_Premultiplied)
    print(bytes(out.constBits()).hex())
"#;

fn hex(px: &[[u8; 4]]) -> String {
    let mut s = String::with_capacity(px.len() * 8);
    for b in px.iter().flatten() {
        write!(s, "{b:02x}").unwrap();
    }
    s
}

/// The alpha of a pixel, from a random number and the pixel's index.
type Alpha = fn(u32, u32) -> u8;

/// The alpha of pixel `k` of a picture of each kind.
const ALPHAS: [(&str, Alpha); 6] = [
    ("opaque", |_, _| 255),
    ("clear", |_, _| 0),
    ("faint", |r, _| (r % 4) as u8),
    ("translucent", |r, _| (1 + r % 254) as u8),
    ("mixed", |r, _| {
        [0, 255, 255, 128, (r % 256) as u8][(r >> 9) as usize % 5]
    }),
    ("edge", |_, k| if k % 3 == 0 { 0 } else { 255 }),
];

#[test]
fn an_argb_image_scales_as_qts() {
    if !std::path::Path::new(PYTHON).exists() {
        testenv::equiv_missing(format_args!("{PYTHON} (PySide6)"));
        return;
    }
    let sizes = [
        (1, 1),
        (2, 3),
        (4, 4),
        (5, 3),
        (7, 5),
        (9, 6),
        (16, 9),
        (23, 31),
    ];
    // Each side to a half and less, between a half and the whole, the same and larger.
    let sides = |n: usize| {
        let mut v = vec![
            n / 3,
            n / 2,
            n / 2 + 1,
            n * 2 / 3,
            n - 1,
            n,
            n + 1,
            n * 3 / 2,
            n * 2,
            n * 3 + 1,
        ];
        v.retain(|d| *d > 0);
        v.dedup();
        v
    };
    let mut cases = Vec::new();
    let mut seed = 0x2545_f491u32;
    for (ws, hs) in sizes {
        let (wide, tall) = (sides(ws), sides(hs));
        for (n, dw) in wide.iter().enumerate() {
            // Both sides by about one factor, and each against another factor of the other.
            for dh in [tall[n.min(tall.len() - 1)], tall[(n * 3 + 1) % tall.len()]] {
                if (*dw, dh) == (ws, hs) {
                    continue;
                }
                for (kind, alpha) in ALPHAS {
                    let px: Vec<[u8; 4]> = (0..(ws * hs) as u32)
                        .map(|k| {
                            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                            let [r, g, b, _] = seed.to_be_bytes();
                            [r, g, b, alpha(seed >> 3, k)]
                        })
                        .collect();
                    cases.push((kind, ws, hs, *dw, dh, px));
                }
            }
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let list = dir.path().join("cases");
    let lines: Vec<String> = cases
        .iter()
        .map(|(_, ws, hs, dw, dh, px)| format!("{ws} {hs} {dw} {dh} {}", hex(px)))
        .collect();
    std::fs::write(&list, lines.join("\n")).unwrap();
    let out = std::process::Command::new(PYTHON)
        .args(["-c", SCALED])
        .arg(&list)
        .env("QT_QPA_PLATFORM", "offscreen")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let qt = String::from_utf8(out.stdout).unwrap();
    let qt: Vec<&str> = qt.lines().collect();
    assert_eq!(qt.len(), cases.len());
    let mut wrong = Vec::new();
    for ((kind, ws, hs, dw, dh, px), qt) in cases.iter().zip(qt) {
        let ours = hex(&qsg::image::scale_straight(px, *ws, *hs, *dw, *dh).px);
        if ours != qt {
            wrong.push(format!("{kind} {ws}x{hs} to {dw}x{dh}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} of {} differ from Qt: {}",
        wrong.len(),
        cases.len(),
        wrong
            .iter()
            .take(40)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    );
}
