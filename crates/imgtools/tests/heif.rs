//! `magick SRC -resize` of AVIF/HEIC: ImageMagick reads them with libheif (coders/heic.c) as
//! plain 8-bit RGB(A), or 16-bit samples shifted up from deeper ones, so the thumbnail must
//! equal that of a PNG holding the same pixels. Fixtures: crates/qsg/tests/fixtures/heif.

use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../qsg/tests/fixtures/heif")
        .join(name)
}

fn write_png(path: &PathBuf, d: heif::Decoded) {
    let mut e = png::Encoder::new(
        std::fs::File::create(path).unwrap(),
        d.width as u32,
        d.height as u32,
    );
    e.set_color(if d.alpha {
        png::ColorType::Rgba
    } else {
        png::ColorType::Rgb
    });
    let data = match d.samples {
        heif::Samples::U8(v) => {
            e.set_depth(png::BitDepth::Eight);
            v
        }
        heif::Samples::U16(v) => {
            e.set_depth(png::BitDepth::Sixteen);
            let shift = 16 - d.bits;
            v.iter().flat_map(|s| (s << shift).to_be_bytes()).collect()
        }
    };
    e.write_header().unwrap().write_image_data(&data).unwrap();
}

#[test]
fn thumbnails_equal_the_decoded_png() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("heif_thumbs");
    std::fs::create_dir_all(&dir).unwrap();
    for name in [
        "quad.avif",
        "quad.heic",
        "alpha.avif",
        "alpha.heic",
        "rot.avif",
        "deep.avif",
    ] {
        let src = fixture(name);
        let d = heif::decode(&std::fs::read(&src).unwrap(), true).unwrap();
        assert_eq!(d.bits, if name == "deep.avif" { 10 } else { 8 }, "{name}");
        let reference = dir.join(format!("{name}.png"));
        write_png(&reference, d);
        for size in [8, 20] {
            let (ours, want) = (dir.join(format!("{name}.{size}.png")), dir.join("want.png"));
            imgtools::magick::resize_to_png(&src, size, &ours)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            imgtools::magick::resize_to_png(&reference, size, &want).unwrap();
            assert_eq!(
                std::fs::read(&ours).unwrap(),
                std::fs::read(&want).unwrap(),
                "{name} at {size}"
            );
        }
    }
}

#[test]
fn cv_reads_heif_as_bgr() {
    let b = imgtools::cv::imread_bgr(fixture("rot.avif").to_str().unwrap()).unwrap();
    if heif::avif::available() {
        // libavif as OpenCV calls it leaves the `irot` rotation alone: tests/formats_cv.rs
        // compares these pixels with cv2.imread's.
        assert_eq!((b.w, b.h), (32, 16));
        return;
    }
    // Without libavif the file is read through libheif, which rotates it.
    assert_eq!((b.w, b.h), (16, 32));
    // Top-left of the rotated quadrants is lime, bottom-right blue (BGR order).
    let at = |x: usize, y: usize| &b.px[3 * (y * b.w + x)..3 * (y * b.w + x) + 3];
    assert!(
        at(4, 4)[1] > 240 && at(4, 4)[0] < 16 && at(4, 4)[2] < 16,
        "{:?}",
        at(4, 4)
    );
    assert!(
        at(11, 27)[0] > 240 && at(11, 27)[1] < 16,
        "{:?}",
        at(11, 27)
    );
}
