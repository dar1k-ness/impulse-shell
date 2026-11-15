//! `cv2.imread` of CMYK / YCCK JPEG and AVIF, in colour and in grey, against OpenCV itself
//! (the opencv wheel of $IMPULSE_SHELL_VIRTUAL_ENV in tools/testenv). Fixtures:
//! crates/qsg/tests/fixtures/{formats,heif}.
use std::path::{Path, PathBuf};
use std::process::Command;

const FILES: [&str; 8] = [
    "formats/cmyk.jpg",
    "formats/ycck.jpg",
    "formats/cmyk_plain.jpg",
    "formats/rgb420.avif",
    "formats/rgba.avif",
    "formats/gray.avif",
    "heif/rot.avif",
    "heif/deep.avif",
];

/// Prints `width height hex` twice per file: IMREAD_COLOR, then IMREAD_GRAYSCALE.
const ORACLE: &str = r#"
import sys
import cv2
for path in sys.argv[1:]:
    for flag in (cv2.IMREAD_COLOR, cv2.IMREAD_GRAYSCALE):
        im = cv2.imread(path, flag)
        print(im.shape[1], im.shape[0], im.tobytes().hex())
"#;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../qsg/tests/fixtures")
        .join(name)
}

fn parse(line: &str) -> (usize, usize, Vec<u8>) {
    let f: Vec<&str> = line.split(' ').collect();
    let px = (0..f[2].len() / 2)
        .map(|i| u8::from_str_radix(&f[2][2 * i..2 * i + 2], 16).unwrap())
        .collect();
    (f[0].parse().unwrap(), f[1].parse().unwrap(), px)
}

#[test]
fn colour_and_grey_equal_opencv() {
    let v = std::env::var("IMPULSE_SHELL_VIRTUAL_ENV").unwrap_or_default();
    let py = format!("{v}/bin/python3");
    let has_cv2 = !v.is_empty()
        && Command::new(&py)
            .args(["-c", "import cv2"])
            .status()
            .is_ok_and(|s| s.success());
    if !has_cv2 {
        return testenv::unavailable("IMPULSE_SHELL_VIRTUAL_ENV with opencv");
    }
    if !heif::avif::available() {
        return testenv::unavailable("libavif (libavif.so.16)");
    }
    let out = Command::new(&py)
        .args(["-c", ORACLE])
        .args(FILES.map(fixture))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = String::from_utf8(out.stdout).unwrap();
    let mut lines = out.lines();
    for name in FILES {
        let bytes = std::fs::read(fixture(name)).unwrap();
        let (w, h, want) = parse(lines.next().unwrap());
        let bgr = imgtools::cv::imdecode_bgr(&bytes).unwrap_or_else(|| panic!("{name}: colour"));
        assert_eq!((bgr.w, bgr.h), (w, h), "{name}: colour size");
        assert!(bgr.px == want, "{name}: colour pixels differ from OpenCV's");

        let (w, h, want) = parse(lines.next().unwrap());
        let gray = imgtools::busy::imdecode_gray(&bytes).unwrap_or_else(|| panic!("{name}: grey"));
        assert_eq!((gray.w, gray.h), (w, h), "{name}: grey size");
        assert!(gray.px == want, "{name}: grey pixels differ from OpenCV's");
    }
}
