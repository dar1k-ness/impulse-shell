//! Equivalence with `text_color.py`: goldens are its stdout (tests/textcolor_golden.py).

use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/textcolor")
}

#[test]
fn matches_script_output() {
    let mut n = 0;
    for e in std::fs::read_dir(dir()).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "json" || x == "txt") {
            continue;
        }
        let golden = std::fs::read_to_string(
            p.with_file_name(format!("{}.json", p.file_name().unwrap().to_str().unwrap())),
        )
        .unwrap();
        let img = imgtools::cv::imdecode_bgr(&std::fs::read(&p).unwrap());
        let got =
            imgtools::textcolor::to_json(img.as_ref().and_then(imgtools::textcolor::text_color));
        assert_eq!(got, golden.trim_end(), "{}", p.display());
        n += 1;
    }
    assert!(n >= 10);
}

#[test]
fn region_matches_magick_crop_pipeline() {
    // `magick F +repage -crop G png:- | text-color-venv.sh` on arch-pc (textcolor_region_golden.sh).
    let mut n = 0;
    for l in include_str!("fixtures/textcolor/regions.txt").lines() {
        let mut p = l.splitn(3, ' ');
        let (f, g, want) = (p.next().unwrap(), p.next().unwrap(), p.next().unwrap());
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
        assert_eq!(got, want, "{l}");
        n += 1;
    }
    assert!(n >= 100);
}

#[test]
fn crop_offsets_truncate_like_magick() {
    // `magick -size 10x10 xc:red +repage -crop G -format '%wx%h' info:` (ImageMagick 7.1.2-35):
    // a negative offset loses its fraction towards zero, it is not rounded down.
    let img = imgtools::cv::Bgr {
        w: 10,
        h: 10,
        px: vec![0; 10 * 10 * 3],
    };
    for (x, y, want) in [
        (-0.5, 0.0, (4, 4)),
        (-1.4, 0.0, (3, 4)),
        (-1.5, 0.0, (3, 4)),
        (-1.6, 0.0, (3, 4)),
        (-2.5, 0.0, (2, 4)),
        (0.0, -0.5, (4, 4)),
        (0.0, -1.5, (4, 3)),
        (1.5, 0.0, (4, 4)),
        (2.5, 1.6, (4, 4)),
    ] {
        let c = imgtools::textcolor::crop(&img, x, y, 4.0, 4.0);
        assert_eq!((c.w, c.h), want, "4x4{x:+}{y:+}");
    }
}
