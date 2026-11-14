//! AVIF/HEIC through `image::load_ex` (libheif): tests/fixtures/heif, made by its make.py.

use qsg::image::load_ex;
use std::path::PathBuf;

fn fixture(name: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/heif")
        .join(name);
    p.to_str().unwrap().to_owned()
}

const RED: [u8; 4] = [255, 0, 0, 255];
const LIME: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];

/// Pixel (x, y) of the premultiplied texture, within 8 of `want` per channel (lossy coding).
#[track_caller]
fn near(name: &str, tex: &qsg::raster::Canvas, x: usize, y: usize, want: [u8; 4]) {
    let got = tex.px[y * tex.w + x];
    assert!(
        got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 8),
        "{name} ({x},{y}): got {got:?}, want ~{want:?}"
    );
}

#[test]
fn opaque_avif_and_heic() {
    for name in ["quad.avif", "quad.heic"] {
        let (tex, alpha) = load_ex(&fixture(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!((tex.w, tex.h, alpha), (32, 16, false), "{name}");
        assert!(tex.px.iter().all(|p| p[3] == 255), "{name}: opaque");
        near(name, &tex, 4, 4, RED);
        near(name, &tex, 27, 4, LIME);
        near(name, &tex, 4, 11, BLUE);
        near(name, &tex, 27, 11, WHITE);
    }
}

#[test]
fn alpha_is_premultiplied() {
    for name in ["alpha.avif", "alpha.heic"] {
        let (tex, alpha) = load_ex(&fixture(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!((tex.w, tex.h, alpha), (32, 16, true), "{name}");
        near(name, &tex, 4, 4, RED);
        near(name, &tex, 4, 12, RED);
        // Lime at alpha 128, premultiplied like QImage::Format_ARGB32_Premultiplied.
        near(name, &tex, 27, 4, [0, 128, 0, 128]);
        let clear = tex.px[12 * tex.w + 27];
        assert_eq!(clear[3], 0, "{name}: transparent corner {clear:?}");
        assert!(
            clear.iter().all(|&c| c == 0),
            "{name}: premultiplied {clear:?}"
        );
        assert!(
            tex.px.iter().all(|p| p[..3].iter().all(|&c| c <= p[3])),
            "{name}: colour above alpha"
        );
    }
}

#[test]
fn irot_is_applied() {
    // quad.avif turned 90 degrees anti-clockwise: lime | white over red | blue.
    let (tex, alpha) = load_ex(&fixture("rot.avif")).unwrap();
    assert_eq!((tex.w, tex.h, alpha), (16, 32, false));
    near("rot.avif", &tex, 4, 4, LIME);
    near("rot.avif", &tex, 11, 4, WHITE);
    near("rot.avif", &tex, 4, 27, RED);
    near("rot.avif", &tex, 11, 27, BLUE);
}

#[test]
fn ten_bit_decodes_to_eight() {
    let (tex, alpha) = load_ex(&fixture("deep.avif")).unwrap();
    assert_eq!((tex.w, tex.h, alpha), (32, 16, false));
    near("deep.avif", &tex, 4, 4, [254, 0, 0, 255]);
    near("deep.avif", &tex, 27, 4, [0, 254, 0, 255]);
    near("deep.avif", &tex, 4, 11, [0, 0, 254, 255]);
    near("deep.avif", &tex, 27, 11, [128, 128, 128, 255]);
}

#[test]
fn detected_by_content_not_name() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("heif_named");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("wallpaper.jpg");
    std::fs::copy(fixture("quad.heic"), &p).unwrap();
    let (tex, _) = load_ex(p.to_str().unwrap()).unwrap();
    assert_eq!((tex.w, tex.h), (32, 16));
    near("wallpaper.jpg", &tex, 27, 11, WHITE);
}

#[test]
fn truncated_file_is_an_error() {
    let data = std::fs::read(fixture("quad.avif")).unwrap();
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("heif_cut");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("cut.avif");
    std::fs::write(&p, &data[..data.len() / 2]).unwrap();
    assert!(load_ex(p.to_str().unwrap()).is_err());
}

/// The same pixels as ImageMagick's libheif decode (its heic coder), for the opaque files.
#[test]
fn matches_imagemagick_decode() {
    let Some(im) = ["magick", "convert"]
        .into_iter()
        .find(|t| testenv::on_path(t))
    else {
        testenv::equiv_missing("ImageMagick (`magick` or `convert`)");
        return;
    };
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("heif_im");
    std::fs::create_dir_all(&dir).unwrap();
    for name in ["quad.avif", "quad.heic", "rot.avif"] {
        let out = dir.join(format!("{name}.rgba"));
        let st = std::process::Command::new(im)
            .arg(fixture(name))
            .args(["-depth", "8"])
            .arg(format!("rgba:{}", out.display()))
            .status()
            .unwrap();
        assert!(st.success(), "{im} failed on {name}");
        let want = std::fs::read(&out).unwrap();
        let (tex, _) = load_ex(&fixture(name)).unwrap();
        let got: Vec<u8> = tex.px.iter().flatten().copied().collect();
        assert_eq!(got.len(), want.len(), "{name}: size");
        let max = got
            .iter()
            .zip(&want)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        assert!(max <= 1, "{name}: max diff {max} against {im}");
    }
}

/// ColorQuantizer's `load_scaled` decodes AVIF/HEIC like `load_ex` (an AVIF wallpaper has a colour).
#[test]
fn load_scaled_reads_heif() {
    for name in ["quad.avif", "quad.heic"] {
        let (tex, alpha, size) =
            qsg::image::load_scaled(&fixture(name), 10.0).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            (tex.w, tex.h, alpha, size),
            (32, 16, false, (32, 16)),
            "{name}"
        );
        near(name, &tex, 4, 4, RED);
        near(name, &tex, 27, 11, WHITE);
    }
}
