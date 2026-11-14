//! The formats `qsg::image` reads without another crate enabling them: this test builds `qsg`
//! alone (`-p qsg`), so every decoder here is one its own Cargo.toml asks for.
use image::{ImageFormat, Rgba, RgbaImage};

fn encoded(format: ImageFormat) -> Vec<u8> {
    let img = RgbaImage::from_fn(3, 2, |x, _| {
        Rgba(if x == 0 {
            [255, 0, 0, 255]
        } else {
            [0, 0, 255, 255]
        })
    });
    let mut img = image::DynamicImage::ImageRgba8(img);
    // The ICO encoder takes RGBA only, the PNM encoder has no alpha.
    if format != ImageFormat::Ico {
        img = img.to_rgb8().into();
    }
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, format).unwrap();
    out.into_inner()
}

#[test]
fn formats_decode_by_content() {
    for format in [
        ImageFormat::Png,
        ImageFormat::Bmp,
        ImageFormat::Gif,
        ImageFormat::Ico,
        ImageFormat::Pnm,
        ImageFormat::Tiff,
        ImageFormat::WebP,
    ] {
        let (img, _) =
            qsg::image::decode_ex(&encoded(format)).unwrap_or_else(|e| panic!("{format:?}: {e}"));
        assert_eq!((img.w, img.h), (3, 2), "{format:?}");
        assert_eq!(img.px[0], [255, 0, 0, 255], "{format:?}");
        assert_eq!(img.px[5], [0, 0, 255, 255], "{format:?}");
    }
}

/// X11 colour names and `None`, as QXpmHandler reads them.
#[test]
fn xpm_decodes() {
    let xpm = b"/* XPM */\nstatic char *x[] = {\n\"3 2 3 1\",\n\"a c #ff0000\",\n\"b c blue\",\n\"  c None\",\n\"ab \",\n\"ab \"\n};\n";
    let (img, alpha) = qsg::image::decode_ex(xpm).unwrap();
    assert_eq!((img.w, img.h, alpha), (3, 2, true));
    assert_eq!(
        img.px[..3],
        [[255, 0, 0, 255], [0, 0, 255, 255], [0, 0, 0, 0]]
    );
}
