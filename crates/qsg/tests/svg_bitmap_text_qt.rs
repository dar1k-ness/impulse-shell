//! `<text>` of an SVG in a bitmap-only font (one 16 px strike, 1 bit a pixel, as a BDF, a PCF
//! and an OpenType file: fixtures/bitmap) against QtSvg itself: tools/oracle/svg_oracle.py renders
//! the same files with QImageReader. SVG text takes its glyph bitmaps by `Font::glyph_xf`.
use std::path::Path;

/// The PySide6 of the testenv image (tools/testenv/Containerfile).
const PYTHON: &str = "/opt/qv4-venv/bin/python";

/// Name, size rendered at (the document is 96x40), body with `FAMILY` for the font's family.
const CASES: &[(&str, usize, usize, &str)] = &[
    (
        "plain",
        96,
        40,
        r##"<text x="3" y="20" font-family="FAMILY" font-size="16" fill="#241f31">ABWijm</text>"##,
    ),
    (
        "each-glyph",
        96,
        40,
        r##"<text x="1" y="16" font-family="FAMILY" font-size="16">m i j</text><text x="1" y="34" font-family="FAMILY" font-size="16" fill="#a51d2d">W B A</text>"##,
    ),
    (
        "part-pixel",
        96,
        40,
        r##"<text x="3.4" y="20.6" font-family="FAMILY" font-size="16" fill="#1a5fb4">mWm</text>"##,
    ),
    (
        "on-colour-half-opaque",
        96,
        40,
        r##"<rect width="96" height="40" fill="#99c1f1"/><text x="3" y="20" font-family="FAMILY" font-size="16" fill="#000" fill-opacity="0.5">mBj</text>"##,
    ),
    (
        "moved",
        96,
        40,
        r##"<g transform="translate(7.3 5.8)"><text x="3" y="20" font-family="FAMILY" font-size="16">mAm</text></g>"##,
    ),
    (
        "twice-the-size",
        192,
        80,
        r##"<text x="3" y="20" font-family="FAMILY" font-size="16" fill="#241f31">mWi</text>"##,
    ),
    (
        "half-the-size",
        48,
        20,
        r##"<text x="3" y="20" font-family="FAMILY" font-size="16" fill="#241f31">ABWijm</text>"##,
    ),
    (
        "turned",
        96,
        40,
        r##"<g transform="rotate(12 48 20)"><text x="3" y="20" font-family="FAMILY" font-size="16">mBm</text></g>"##,
    ),
    (
        "no-strike-of-the-size",
        96,
        40,
        r##"<text x="3" y="20" font-family="FAMILY" font-size="11">ABWijm</text>"##,
    ),
];

/// Qt's premultiplied RGBA pixels of every `(w, h, file)`.
fn qt_render(dir: &Path, files: &[(usize, usize, std::path::PathBuf)]) -> Vec<Vec<[u8; 4]>> {
    let list: String = files
        .iter()
        .map(|(w, h, f)| format!("{w} {h} {}\n", f.display()))
        .collect();
    let (list_p, out_p) = (dir.join("list"), dir.join("qt.rgba"));
    std::fs::write(&list_p, list).unwrap();
    let oracle = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/oracle/svg_oracle.py");
    let st = std::process::Command::new(PYTHON)
        .arg(oracle)
        .args([&out_p, &list_p])
        .env("QT_QPA_PLATFORM", "offscreen")
        .status()
        .unwrap();
    assert!(st.success(), "svg_oracle.py failed");
    let raw = std::fs::read(&out_p).unwrap();
    let mut at = 0;
    let out = files
        .iter()
        .map(|(w, h, _)| {
            let n = w * h * 4;
            let px = raw[at..at + n].as_chunks::<4>().0.to_vec();
            at += n;
            px
        })
        .collect();
    assert_eq!(at, raw.len());
    out
}

/// Every case, in a process whose fontconfig has the fixture `file`
/// and no other user font, has Qt's pixels; Qt's own picture of each has ink, so the family
/// asked for is the one drawn.
fn compare(test: &str, file: &str, family: &str) {
    if !Path::new(PYTHON).exists() {
        testenv::equiv_missing(format_args!("{PYTHON} (PySide6)"));
        return;
    }
    if !testenv::in_own_process(test) {
        let dir = tempfile::tempdir().unwrap();
        let (data, cache) = (dir.path().join("data"), dir.path().join("cache"));
        std::fs::create_dir_all(data.join("fonts")).unwrap();
        let fixture = format!(
            "{}/tests/fixtures/bitmap/{file}",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::copy(fixture, data.join("fonts").join(file)).unwrap();
        testenv::run_in_own_process(test, [("XDG_DATA_HOME", data), ("XDG_CACHE_HOME", cache)]);
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let files: Vec<_> = CASES
        .iter()
        .map(|(name, w, h, body)| {
            let path = dir.path().join(format!("{name}.svg"));
            let svg = format!(
                r##"<svg xmlns="http://www.w3.org/2000/svg" width="96" height="40" viewBox="0 0 96 40">{}</svg>"##,
                body.replace("FAMILY", family)
            );
            std::fs::write(&path, svg).unwrap();
            (*w, *h, path)
        })
        .collect();
    let qt = qt_render(dir.path(), &files);
    let mut wrong = Vec::new();
    for (((name, ..), (w, h, path)), qt) in CASES.iter().zip(&files).zip(&qt) {
        let doc = qsg::svg::Doc::load(path.to_str().unwrap()).unwrap();
        let ours = doc.render(*w, *h).unwrap();
        let ink = qt.iter().filter(|p| p[3] != 0).count();
        let (mut n, mut max) = (0, 0);
        for (a, b) in ours.px.iter().zip(qt) {
            let d = (0..4).map(|k| a[k].abs_diff(b[k])).max().unwrap();
            n += (d > 0) as usize;
            max = max.max(d);
        }
        eprintln!("{file} {name}: Qt draws {ink} pixels, {n} differ, by up to {max}");
        if n != 0 || ink == 0 {
            wrong.push(format!(
                "{name}: Qt draws {ink} pixels; {n} of {} differ, by up to {max}",
                w * h
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "{file}: {} of {} cases are not drawn as Qt draws them:\n{}",
        wrong.len(),
        CASES.len(),
        wrong.join("\n")
    );
}

#[test]
fn text_in_a_bdf_font_matches_qt() {
    const TEST: &str = concat!(module_path!(), "::text_in_a_bdf_font_matches_qt");
    compare(TEST, "impulse-bitmap-test.bdf", "Impulse Bitmap Test");
}

/// FreeType names a PCF file's family with its foundry first.
#[test]
fn text_in_a_pcf_font_matches_qt() {
    const TEST: &str = concat!(module_path!(), "::text_in_a_pcf_font_matches_qt");
    compare(
        TEST,
        "impulse-bitmap-test.pcf",
        "impulse Impulse Bitmap Test",
    );
}

#[test]
fn text_in_an_otb_font_matches_qt() {
    const TEST: &str = concat!(module_path!(), "::text_in_an_otb_font_matches_qt");
    compare(TEST, "impulse-bitmap-test.otb", "Impulse Bitmap Test");
}
