//! Palette equivalence: `generate_colors_material.py` (materialyoucolor 2.0.10) SCSS for 20
//! seeds x 10 schemes x 2 modes, matugen 4.2.0 `colors.json` for the same, and
//! `scheme_for_image.py` colorfulness + image seeds on the synthetic images.

use colorgen::mg::{Colors, render, variant};
use colorgen::scss::{TermOpts, generate, parse_hex};
use std::path::Path;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/palette");
const TERM: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../qml/scripts/colors/terminal/scheme-base.json"
);

fn term() -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(TERM).unwrap()).unwrap()
}

fn names(dir: &str) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(format!("{DIR}/{dir}"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    v.sort();
    v
}

#[test]
fn scss_matches_python() {
    let term = term();
    let opts = TermOpts {
        blend_bg_fg: true,
        ..Default::default()
    };
    let names = names("scss");
    assert_eq!(names.len(), 400);
    let bad: Vec<&String> = names
        .iter()
        .filter(|n| {
            // c_<HEX>_<scheme>_<mode>.scss
            let stem = n.trim_end_matches(".scss");
            let (hex, rest) = stem[2..].split_once('_').unwrap();
            let (scheme, mode) = rest.rsplit_once('_').unwrap();
            let got = generate(
                parse_hex(&format!("#{hex}")).unwrap(),
                scheme,
                mode == "dark",
                Some(&term),
                &opts,
            )
            .unwrap_or_default();
            got != std::fs::read_to_string(format!("{DIR}/scss/{n}")).unwrap()
        })
        .collect();
    assert!(bad.is_empty(), "{} differ: {bad:?}", bad.len());
}

#[test]
fn colors_json_matches_matugen() {
    let template = std::fs::read_to_string(format!("{DIR}/mg-template.json")).unwrap();
    let names = names("mg");
    assert_eq!(names.len(), 400);
    let bad: Vec<&String> = names
        .iter()
        .filter(|n| {
            // c_<HEX>_<none|scheme-*>_<mode>.json
            let stem = n.trim_end_matches(".json");
            let (hex, rest) = stem[2..].split_once('_').unwrap();
            let (ty, mode) = rest.rsplit_once('_').unwrap();
            let ty = (ty != "none").then_some(ty);
            let colors = Colors::new(
                parse_hex(&format!("#{hex}")).unwrap(),
                variant(ty),
                mode == "dark",
            );
            render(&template, &colors, None).unwrap()
                != std::fs::read_to_string(format!("{DIR}/mg/{n}")).unwrap()
        })
        .collect();
    assert!(bad.is_empty(), "{} differ: {bad:?}", bad.len());
}

#[test]
fn images_match_python() {
    let term = term();
    let opts = TermOpts {
        blend_bg_fg: true,
        ..Default::default()
    };
    let cf: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(format!("{DIR}/img/colorfulness.json")).unwrap(),
    )
    .unwrap();
    let imgs: Vec<String> = names("img")
        .into_iter()
        .filter(|n| n.starts_with("syn_"))
        .collect();
    assert_eq!(imgs.len(), 7);
    for n in imgs {
        let path = Path::new(DIR).join("img").join(&n);
        let bytes = std::fs::read(&path).unwrap();
        let want: f64 = cf[&n].as_str().unwrap().parse().unwrap();
        let img = colorgen::colorfulness::load_and_resize(&bytes).unwrap();
        assert_eq!(
            colorgen::colorfulness::colorfulness(&img),
            want,
            "{n} colorfulness"
        );
        let expect = if want < 40.0 {
            "scheme-neutral"
        } else {
            "scheme-tonal-spot"
        };
        assert_eq!(
            colorgen::colorfulness::scheme_for_image(&path),
            expect,
            "{n} scheme"
        );
        let seed = colorgen::seed::seed_from_bytes(&bytes).unwrap();
        let hex = format!("#{:02X}{:02X}{:02X}", seed.red, seed.green, seed.blue);
        assert_eq!(
            hex,
            std::fs::read_to_string(format!("{DIR}/img/i_{n}.color")).unwrap(),
            "{n} seed"
        );
        let scss = generate(seed, "scheme-tonal-spot", true, Some(&term), &opts).unwrap();
        assert_eq!(
            scss,
            std::fs::read_to_string(format!("{DIR}/img/i_{n}.scss")).unwrap(),
            "{n} scss"
        );
    }
}
