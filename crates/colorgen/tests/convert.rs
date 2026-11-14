//! The wallpaper's pixels as the two original scripts read them, on files that Pillow and
//! OpenCV bring to 8 bits, turn or flatten each in its own way (16 bits per sample, an
//! Orientation tag, a `tRNS` chunk). Compared live with `generate_colors_material.py` and
//! `scheme_for_image.py` under the Python of `$RUSTY_IMPULSE_VIRTUAL_ENV` (Pillow 13.0.0,
//! OpenCV 4.13.0 in ri-testenv). The files are imgtools' tests/fixtures/convert (its make.py).

use colorgen::{colorfulness, seed};
use std::path::PathBuf;
use std::process::{Command, Stdio};

const SCRIPTS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../qml/tests/fixtures/original-unshipped/scripts/colors"
);

fn file(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../imgtools/tests/fixtures/convert")
        .join(name)
}

/// The venv's Python, with Pillow, materialyoucolor and OpenCV.
fn python() -> Option<String> {
    let venv = std::env::var("RUSTY_IMPULSE_VIRTUAL_ENV").unwrap_or_default();
    let py = format!("{venv}/bin/python3");
    let ok = !venv.is_empty()
        && Command::new(&py)
            .args(["-c", "import cv2, numpy, PIL, materialyoucolor"])
            .status()
            .is_ok_and(|s| s.success());
    if !ok {
        testenv::unavailable("RUSTY_IMPULSE_VIRTUAL_ENV with pillow, materialyoucolor, cv2");
        return None;
    }
    Some(py)
}

/// Files the colour script reads into 3 or 4 bands. (It dies on a 16-bit grey one, mode
/// `I;16`, and takes two-band tuples from grey with alpha.)
const SEED_FILES: [&str; 20] = [
    "rgb16.png",
    "rgba16.png",
    "graya16.png",
    "rgb16_trns.png",
    "rgb16.tiff",
    "rgba16.tiff",
    "rgba16_assoc.tiff",
    "exif3.tiff",
    "exif6.tiff",
    "pal_trns.png",
    "gray_trns.png",
    "rgb_trns.png",
    "rgb.tiff",
    "rgba.tiff",
    "rgba_assoc.tiff",
    // Pillow turns none of these: the script never asks for the EXIF turn.
    "exif6.jpg",
    "exif6.png",
    "exif6.webp",
    "rgb.webp",
    "rgba.webp",
];

/// The image as generate_colors_material.py holds it before its resize: for every file of
/// argv[1..], width, height and bands as three little-endian u32, then the pixels.
const PILLOW: &str = r#"
import struct, sys
from PIL import Image
for f in sys.argv[1:]:
    image = Image.open(f)
    if image.mode in ["L", "P"]:
        image = image.convert('RGB')
    w, h = image.size
    sys.stdout.buffer.write(struct.pack("<III", w, h, len(image.getbands())) + image.tobytes())
"#;

/// `(width, height, channels, pixels)` of each image a script wrote to stdout.
fn images(mut raw: &[u8], n: usize) -> Vec<(usize, usize, usize, Vec<u8>)> {
    let u32le = |raw: &mut &[u8]| {
        let (v, rest) = raw.split_first_chunk::<4>().unwrap();
        *raw = rest;
        u32::from_le_bytes(*v) as usize
    };
    let all = (0..n)
        .map(|_| {
            let (w, h, c) = (u32le(&mut raw), u32le(&mut raw), u32le(&mut raw));
            let (px, rest) = raw.split_at(w * h * c);
            raw = rest;
            (w, h, c, px.to_vec())
        })
        .collect();
    assert!(raw.is_empty());
    all
}

/// "N bytes differ, by up to M", or `None` for equal pixels.
fn difference(got: &[u8], want: &[u8]) -> Option<String> {
    let diff = |(a, b): (&u8, &u8)| a.abs_diff(*b);
    let n = got.iter().zip(want).filter(|&p| diff(p) > 0).count();
    let max = got.iter().zip(want).map(diff).max().unwrap_or(0);
    (n > 0).then(|| format!("{n} bytes differ, by up to {max}"))
}

#[test]
fn pixels_for_the_seed_are_pillows() {
    let Some(py) = python() else { return };
    let out = Command::new(&py)
        .args(["-c", PILLOW])
        .args(SEED_FILES.map(file))
        .stderr(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success());
    let mut bad = Vec::new();
    for (f, (w, h, c, want)) in SEED_FILES.iter().zip(images(&out.stdout, SEED_FILES.len())) {
        let got = seed::decode(&std::fs::read(file(f)).unwrap()).unwrap();
        if (got.width, got.height, got.channels) != (w, h, c) {
            bad.push(format!(
                "{f}: {}x{} of {} channels, Pillow {w}x{h} of {c}",
                got.width, got.height, got.channels
            ));
        } else if let Some(d) = difference(&got.data, &want) {
            bad.push(format!("{f}: {d}"));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn seed_is_the_scripts() {
    let Some(py) = python() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let mut bad = Vec::new();
    for f in SEED_FILES {
        let cache = tmp.path().join("color");
        let st = Command::new(&py)
            .arg("-E")
            .arg(format!("{SCRIPTS}/generate_colors_material.py"))
            .arg("--path")
            .arg(file(f))
            .arg("--cache")
            .arg(&cache)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(st.success(), "{f}: the script failed");
        let want = std::fs::read_to_string(&cache).unwrap();
        let seed = seed::seed_from_path(&file(f)).unwrap();
        let got = format!("#{:02X}{:02X}{:02X}", seed.red, seed.green, seed.blue);
        if got != want {
            bad.push(format!("{f}: {got}, script {want}"));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

/// Files `cv2.imread` reads. OpenCV 4.13.0 refuses exif6.tiff, a TIFF whose Orientation swaps
/// the sides ("Internal imread issue"; `cv2.imdecode` turns it), so the script only prints its
/// fallback for it.
const CV_FILES: [&str; 23] = [
    "exif3.jpg",
    "exif6.jpg",
    "exif6.png",
    "exif5.png",
    "exif6.webp",
    "exif8.webp",
    "exif3.tiff",
    "rgb16.png",
    "rgba16.png",
    "gray16.png",
    "graya16.png",
    "rgb16_trns.png",
    "rgb16.tiff",
    "rgba16.tiff",
    "rgba16_assoc.tiff",
    "gray16.tiff",
    "pal_trns.png",
    "rgb.webp",
    "rgba.webp",
    "rgb.bmp",
    "rgb.tiff",
    "rgba.tiff",
    "rgba_assoc.tiff",
];

/// What `scheme_for_image.py ARGS FILE` prints.
fn script(py: &str, args: &[&str], f: &str) -> String {
    let out = Command::new(py)
        .arg("-E")
        .arg(format!("{SCRIPTS}/scheme_for_image.py"))
        .args(args)
        .arg(file(f))
        .stderr(Stdio::null())
        .output()
        .unwrap();
    String::from_utf8(out.stdout)
        .unwrap()
        .trim_end()
        .to_string()
}

#[test]
fn colourfulness_is_the_scripts() {
    let Some(py) = python() else { return };
    let mut bad = Vec::new();
    for f in CV_FILES {
        let want = script(&py, &["--colorfulness"], f);
        let img = colorfulness::load_and_resize(&std::fs::read(file(f)).unwrap()).unwrap();
        let got = colorfulness::colorfulness(&img);
        if want.parse::<f64>() != Ok(got) {
            bad.push(format!("{f}: {got}, script {want}"));
        }
        assert_eq!(
            colorfulness::scheme_for_image(&file(f)),
            script(&py, &[], f),
            "{f}"
        );
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

/// `cv2.imdecode(…, IMREAD_COLOR)` of every file of argv[1..], as [`PILLOW`] writes them.
const CV2: &str = r#"
import struct, sys
import cv2, numpy as np
for f in sys.argv[1:]:
    img = cv2.imdecode(np.fromfile(f, np.uint8), cv2.IMREAD_COLOR)
    h, w = img.shape[:2]
    sys.stdout.buffer.write(struct.pack("<III", w, h, 3) + img.tobytes())
"#;

#[test]
fn imread_is_what_opencv_decodes() {
    let Some(py) = python() else { return };
    let files: Vec<&str> = CV_FILES.into_iter().chain(["exif6.tiff"]).collect();
    let out = Command::new(&py)
        .args(["-c", CV2])
        .args(files.iter().map(|f| file(f)))
        .stderr(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success());
    let mut bad = Vec::new();
    for (f, (w, h, _, want)) in files.iter().zip(images(&out.stdout, files.len())) {
        let got = colorfulness::imread(&std::fs::read(file(f)).unwrap()).unwrap();
        if (got.width, got.height) != (w, h) {
            bad.push(format!("{f}: {}x{}, OpenCV {w}x{h}", got.width, got.height));
        } else if let Some(d) = difference(&got.data, &want) {
            bad.push(format!("{f}: {d}"));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}
