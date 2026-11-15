//! AVIF and CMYK / YCCK JPEG wallpapers against the original scripts and the libraries they
//! read images with (Pillow, OpenCV; tools/testenv has them in $IMPULSE_SHELL_VIRTUAL_ENV):
//! the decoded pixels, the source colour of generate_colors_material.py and the colourfulness
//! of scheme_for_image.py. Fixtures: crates/qsg/tests/fixtures/{formats,heif}.
use std::path::{Path, PathBuf};
use std::process::Command;

/// CMYK and YCCK JPEG (with and without the Adobe marker), AVIF 4:2:0, with alpha, 4:0:0,
/// with an `irot` rotation (which neither library applies) and 10-bit.
const FILES: [&str; 9] = [
    "formats/cmyk.jpg",
    "formats/ycck.jpg",
    "formats/cmyk_plain.jpg",
    "formats/rgb420.avif",
    "formats/rgba.avif",
    "formats/gray.avif",
    "heif/rot.avif",
    "heif/deep.avif",
    "heif/alpha.avif",
];

/// Prints `width height bands hex` twice per file: the pixels the colour script quantizes
/// (Pillow, modes "L" and "P" converted to "RGB" as it does) and `cv2.imread`'s.
const ORACLE: &str = r#"
import sys
import cv2
from PIL import Image
for path in sys.argv[1:]:
    im = Image.open(path)
    if im.mode in ["L", "P"]:
        im = im.convert("RGB")
    print(im.width, im.height, len(im.getbands()), im.tobytes().hex())
    bgr = cv2.imread(path)
    print(bgr.shape[1], bgr.shape[0], bgr.shape[2], bgr.tobytes().hex())
"#;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../qsg/tests/fixtures")
        .join(name)
}

fn script(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../qml/tests/fixtures/original-unshipped/scripts/colors")
        .join(name)
}

/// The scripts' python; `None` (reported) without it or its libraries.
fn python() -> Option<String> {
    let v = std::env::var("IMPULSE_SHELL_VIRTUAL_ENV").unwrap_or_default();
    let py = format!("{v}/bin/python3");
    let ok = !v.is_empty()
        && Command::new(&py)
            .args(["-c", "import materialyoucolor, PIL, cv2"])
            .status()
            .is_ok_and(|s| s.success());
    if !ok {
        testenv::unavailable("IMPULSE_SHELL_VIRTUAL_ENV with materialyoucolor, pillow, opencv");
        return None;
    }
    if !heif::avif::available() {
        testenv::unavailable("libavif (libavif.so.16)");
        return None;
    }
    Some(py)
}

fn stdout(c: &mut Command) -> String {
    let out = c.output().unwrap();
    assert!(
        out.status.success(),
        "{c:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

struct Raw {
    w: usize,
    h: usize,
    bands: usize,
    px: Vec<u8>,
}

fn raw(line: &str) -> Raw {
    let f: Vec<&str> = line.split(' ').collect();
    let px = (0..f[3].len() / 2)
        .map(|i| u8::from_str_radix(&f[3][2 * i..2 * i + 2], 16).unwrap())
        .collect();
    Raw {
        w: f[0].parse().unwrap(),
        h: f[1].parse().unwrap(),
        bands: f[2].parse().unwrap(),
        px,
    }
}

#[test]
fn cmyk_jpeg_pixels_equal_pillow_and_opencv() {
    pixels_equal_pillow_and_opencv(&FILES[..3]);
}

#[test]
fn avif_pixels_equal_pillow_and_opencv() {
    pixels_equal_pillow_and_opencv(&FILES[3..]);
}

fn pixels_equal_pillow_and_opencv(files: &[&str]) {
    let Some(py) = python() else { return };
    let out = stdout(
        Command::new(py)
            .args(["-c", ORACLE])
            .args(files.iter().map(|f| fixture(f))),
    );
    let mut lines = out.lines();
    for &name in files {
        let bytes = std::fs::read(fixture(name)).unwrap();
        let (pil, cv) = (raw(lines.next().unwrap()), raw(lines.next().unwrap()));

        let px = colorgen::seed::decode(&bytes).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert_eq!((px.width, px.height), (pil.w, pil.h), "{name}: Pillow size");
        // Of a "CMYK" image the quantizer reads C, M and Y; `decode` drops K.
        let cmyk = name.ends_with(".jpg");
        assert_eq!(pil.bands, if cmyk { 4 } else { px.channels }, "{name}");
        let same = px
            .data
            .chunks_exact(px.channels)
            .zip(pil.px.chunks_exact(pil.bands))
            .filter(|(a, b)| a[..] != b[..px.channels])
            .count();
        assert_eq!(same, 0, "{name}: pixels that differ from Pillow's");

        let bgr =
            colorgen::colorfulness::imread(&bytes).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert_eq!((bgr.width, bgr.height, 3), (cv.w, cv.h, cv.bands), "{name}");
        let (ours, theirs) = (bgr.data.as_chunks::<3>().0, cv.px.as_chunks::<3>().0);
        let differ = ours.iter().zip(theirs).filter(|(a, b)| a != b).count();
        assert_eq!(differ, 0, "{name}: pixels that differ from cv2.imread's");
    }
}

#[test]
fn colour_and_colourfulness_equal_the_scripts() {
    let Some(py) = python() else { return };
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("color.txt");
    for name in FILES {
        let path = fixture(name);
        stdout(
            Command::new(&py)
                .arg(script("generate_colors_material.py"))
                .arg("--path")
                .arg(&path)
                .arg("--cache")
                .arg(&cache),
        );
        let seed = colorgen::seed::seed_from_path(&path).unwrap();
        assert_eq!(
            format!("#{:02X}{:02X}{:02X}", seed.red, seed.green, seed.blue),
            std::fs::read_to_string(&cache).unwrap(),
            "{name}: source colour"
        );

        let want = stdout(
            Command::new(&py)
                .arg(script("scheme_for_image.py"))
                .arg("--colorfulness")
                .arg(&path),
        );
        let img = colorgen::colorfulness::load_and_resize(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            colorgen::colorfulness::colorfulness(&img),
            want.trim().parse::<f64>().unwrap(),
            "{name}: colourfulness"
        );
    }
}

/// matugen 4.2.0 reads neither format (the script ignores its failure); the port gives its
/// templates the colour of the same pixels saved as PNG.
#[test]
fn matugen_source_reads_avif_and_heic() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["formats/rgb420.avif", "formats/rgba.avif", "heif/quad.heic"] {
        let path = fixture(name);
        let px = colorgen::seed::decode(&std::fs::read(&path).unwrap()).unwrap();
        let png = dir.path().join("same.png");
        let (w, h) = (px.width as u32, px.height as u32);
        if px.channels == 4 {
            image::RgbaImage::from_raw(w, h, px.data)
                .unwrap()
                .save(&png)
        } else {
            image::RgbImage::from_raw(w, h, px.data).unwrap().save(&png)
        }
        .unwrap();
        assert_eq!(
            colorgen::mg::source_from_image(&path).unwrap_or_else(|e| panic!("{name}: {e:?}")),
            colorgen::mg::source_from_image(&png).unwrap(),
            "{name}"
        );
    }
}

/// HEIC, which neither Pillow nor OpenCV reads, still gives a colour and a colourfulness.
#[test]
fn heic_gives_a_colour() {
    let bytes = std::fs::read(fixture("heif/quad.heic")).unwrap();
    colorgen::seed::seed_from_bytes(&bytes).unwrap();
    let img = colorgen::colorfulness::load_and_resize(&bytes).unwrap();
    assert!(colorgen::colorfulness::colorfulness(&img) > 40.0);
}
