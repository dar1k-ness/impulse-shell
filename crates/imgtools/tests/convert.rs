//! Files that every reader brings to 8 bits, turns or flattens in its own way (16 bits per
//! sample, an Orientation tag, a `tRNS` chunk, alpha in a TIFF, colour read as grey), against
//! OpenCV itself and against ScreenTextOverlay's real `magick … | text_color.py` pipeline, run
//! live: the Python of `$RUSTY_IMPULSE_VIRTUAL_ENV` (OpenCV 4.13.0 in ri-testenv) and
//! ImageMagick. The files are tests/fixtures/convert, written by its make.py.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/convert")
}

const FILES: [&str; 28] = [
    "rgb16.png",
    "rgba16.png",
    "gray16.png",
    "graya16.png",
    "rgb16_trns.png",
    "rgb16.tiff",
    "rgba16.tiff",
    "rgba16_assoc.tiff",
    "gray16.tiff",
    "exif3.jpg",
    "exif6.jpg",
    "exif6.png",
    "exif5.png",
    "exif6.webp",
    "exif8.webp",
    "exif3.tiff",
    "exif6.tiff",
    "pal_trns.png",
    "gray_trns.png",
    "rgb_trns.png",
    "rgb.webp",
    "rgba.webp",
    "rgb_lossless.webp",
    "rgb.bmp",
    "rgb.gif",
    "rgb.tiff",
    "rgba.tiff",
    "rgba_assoc.tiff",
];

/// The venv's Python, with OpenCV.
fn python() -> Option<String> {
    let venv = std::env::var("RUSTY_IMPULSE_VIRTUAL_ENV").unwrap_or_default();
    let py = format!("{venv}/bin/python3");
    let ok = !venv.is_empty()
        && Command::new(&py)
            .args(["-c", "import cv2, numpy"])
            .status()
            .is_ok_and(|s| s.success());
    if !ok {
        testenv::unavailable("RUSTY_IMPULSE_VIRTUAL_ENV with cv2 + numpy");
        return None;
    }
    Some(py)
}

/// For every file of argv[2..]: width, height and channels as three little-endian u32 (all 0
/// when OpenCV reads nothing), then the pixels. argv[1] is `imdecode` or `imread` and the
/// `cv2.IMREAD_*` flag.
const CV2: &str = r#"
import struct, sys
import cv2, numpy as np
how, flag = sys.argv[1].split(":")
flag = getattr(cv2, flag)
for f in sys.argv[2:]:
    img = cv2.imread(f, flag) if how == "imread" else cv2.imdecode(np.fromfile(f, np.uint8), flag)
    if img is None:
        sys.stdout.buffer.write(struct.pack("<III", 0, 0, 0))
        continue
    h, w = img.shape[:2]
    sys.stdout.buffer.write(struct.pack("<III", w, h, img.size // (w * h)) + img.tobytes())
"#;

struct Read {
    w: usize,
    h: usize,
    px: Vec<u8>,
}

/// What OpenCV reads from every file of [`FILES`] (`None` where it reads nothing).
fn cv2(py: &str, how: &str) -> Vec<Option<Read>> {
    let out = Command::new(py)
        .args(["-c", CV2, how])
        .args(FILES.iter().map(|f| dir().join(f)))
        .stderr(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success(), "cv2 {how} failed");
    let mut raw = &out.stdout[..];
    let u32le = |raw: &mut &[u8]| {
        let (v, rest) = raw.split_first_chunk::<4>().unwrap();
        *raw = rest;
        u32::from_le_bytes(*v) as usize
    };
    let all = FILES
        .iter()
        .map(|_| {
            let (w, h, c) = (u32le(&mut raw), u32le(&mut raw), u32le(&mut raw));
            if w == 0 {
                return None;
            }
            let (px, rest) = raw.split_at(w * h * c);
            raw = rest;
            Some(Read {
                w,
                h,
                px: px.to_vec(),
            })
        })
        .collect();
    assert!(raw.is_empty());
    all
}

/// The files where `ours` is not what OpenCV read, each with its size and how many bytes
/// differ by how much at most.
fn differing(want: &[Option<Read>], ours: impl Fn(&[u8]) -> Option<Read>) -> Vec<String> {
    FILES
        .iter()
        .zip(want)
        .filter_map(|(f, want)| {
            let got = ours(&std::fs::read(dir().join(f)).unwrap());
            let size = |r: &Option<Read>| r.as_ref().map(|r| (r.w, r.h));
            if size(&got) != size(want) {
                return Some(format!("{f}: {:?}, OpenCV {:?}", size(&got), size(want)));
            }
            let (got, want) = (got?, want.as_ref()?);
            let diff = |(a, b): (&u8, &u8)| a.abs_diff(*b);
            let n = got.px.iter().zip(&want.px).filter(|&p| diff(p) > 0).count();
            let max = got.px.iter().zip(&want.px).map(diff).max().unwrap_or(0);
            (n > 0).then(|| format!("{f}: {n} bytes differ, by up to {max}"))
        })
        .collect()
}

#[test]
fn colour_is_what_opencv_decodes() {
    let Some(py) = python() else { return };
    let want = cv2(&py, "imdecode:IMREAD_COLOR");
    assert!(want.iter().all(Option::is_some));
    let bad = differing(&want, |b| {
        imgtools::cv::imdecode_bgr(b).map(|i| Read {
            w: i.w,
            h: i.h,
            px: i.px,
        })
    });
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn grey_is_what_opencv_decodes() {
    let Some(py) = python() else { return };
    let want = cv2(&py, "imdecode:IMREAD_GRAYSCALE");
    assert!(want.iter().all(Option::is_some));
    let bad = differing(&want, |b| {
        imgtools::busy::imdecode_gray(b).map(|i| Read {
            w: i.w,
            h: i.h,
            px: i.px,
        })
    });
    assert!(bad.is_empty(), "{bad:#?}");
}

/// `cv2.imread`, which the scripts that take a path call, reads what `cv2.imdecode` reads but
/// for one case in OpenCV 4.13.0: a TIFF whose Orientation swaps the sides is refused ("Internal
/// imread issue"), where `imdecode` turns it. Ours reads it, turned.
#[test]
fn imread_differs_from_imdecode_only_on_a_tiff_turned_sideways() {
    let Some(py) = python() else { return };
    let (read, decoded) = (
        cv2(&py, "imread:IMREAD_COLOR"),
        cv2(&py, "imdecode:IMREAD_COLOR"),
    );
    for ((f, read), decoded) in FILES.iter().zip(&read).zip(&decoded) {
        let decoded = decoded.as_ref().unwrap();
        match read {
            Some(read) => assert!(
                (read.w, read.h) == (decoded.w, decoded.h) && read.px == decoded.px,
                "{f}"
            ),
            None => assert_eq!(*f, "exif6.tiff"),
        }
    }
}

/// What `magick F +repage -crop G png:- | text_color.py` prints (ScreenTextOverlay.qml's
/// command; text-color-venv.sh only activates the venv around the script).
fn pipeline(py: &str, file: &str, geometry: &str) -> String {
    let crop = Command::new("magick")
        .arg(dir().join(file))
        .args(["+repage", "-crop", geometry, "png:-"])
        .stderr(Stdio::null())
        .output()
        .unwrap();
    let script = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../qml/tests/fixtures/original-unshipped/scripts/images/text_color.py"
    );
    let mut child = Command::new(py)
        .args(["-E", script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(&crop.stdout).unwrap();
    let out = child.wait_with_output().unwrap();
    String::from_utf8(out.stdout)
        .unwrap()
        .trim_end()
        .to_string()
}

/// `magick` crops the pixels as the file stores them; the EXIF block it copies into its PNG
/// then turns the crop in OpenCV. So the region is counted in the stored image, not in the
/// turned one.
#[test]
fn region_of_a_turned_file_is_the_pipelines() {
    let Some(py) = python() else { return };
    if !testenv::require("magick") {
        return;
    }
    let mut bad = Vec::new();
    for f in [
        "exif3.jpg",
        "exif6.jpg",
        "exif6.png",
        "exif5.png",
        "exif6.webp",
        "exif8.webp",
        "exif3.tiff",
        "exif6.tiff",
        "rgb16.png",
        "rgb16.tiff",
        "rgba16.tiff",
        "rgba.tiff",
        "rgba.webp",
        "rgb.gif",
    ] {
        // The 16-bit and alpha files are 48x32, the others 210x140.
        for g in [
            "60x40+10+4",
            "50x50+120+85",
            "70x45+100+0",
            "30x0+150+20",
            "0x0+3+2",
            "20x12+4+3",
            "9.5x30.5+30.5+0.4",
        ] {
            let (wh, xy) = g.split_once('+').unwrap();
            let (w, h) = wh.split_once('x').unwrap();
            let (x, y) = xy.split_once('+').unwrap();
            let num = |s: &str| s.parse::<f64>().unwrap();
            let got = imgtools::textcolor::region_json(
                dir().join(f).to_str().unwrap(),
                num(x),
                num(y),
                num(w),
                num(h),
            );
            let want = pipeline(&py, f, g);
            assert!(want.contains("background"), "{f} {g}: {want}");
            if got != want {
                bad.push(format!("{f} {g}: {got}, pipeline {want}"));
            }
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}
