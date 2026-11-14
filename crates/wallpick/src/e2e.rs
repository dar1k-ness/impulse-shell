//! End to end with the real MobileCLIP model (`cargo test -p wallpick -- --ignored`).
//!
//! Uses `WALLPICK_CACHE` as the cache dir when set (a dir that already holds
//! `models/mobileclip_s0-<rev>`), else downloads the ~220 MB of model files into a temp dir.
//! References come from onnxruntime 1.31.1 on the same ONNX files and the official tokenizer.

use super::*;
use std::collections::HashMap;

fn cache() -> (Option<tempfile::TempDir>, PathBuf) {
    match std::env::var_os("WALLPICK_CACHE") {
        Some(p) => (None, PathBuf::from(p)),
        None => {
            let t = tempfile::tempdir().unwrap();
            let p = t.path().to_path_buf();
            (Some(t), p)
        }
    }
}

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

fn cos(a: &[f32], b: &[f32]) -> f32 {
    dot(a, b) / (dot(a, a).sqrt() * dot(b, b).sqrt())
}

#[test]
#[ignore = "downloads and runs the real model (~220 MB)"]
fn real_model_end_to_end() {
    let (_keep, cache) = cache();
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs {
        state: tmp.path().join("state"),
        cache: cache.clone(),
        downloads: tmp.path().join("dl"),
    };
    let cancel = Cancel::new();
    let clip = Clip::new(&cache, &model::endpoint(), &cancel);

    let tok_path = model::ensure(
        &model::dir(&cache),
        &model::endpoint(),
        &model::TOKENIZER,
        &cancel,
    )
    .unwrap();
    let tok = tokenizer::Tokenizer::from_json(&std::fs::read(tok_path).unwrap()).unwrap();
    tokenizer::tests::check_reference(&tok);

    #[derive(serde::Deserialize)]
    struct Refs {
        text: HashMap<String, Vec<f32>>,
        image: HashMap<String, Vec<f32>>,
    }
    let refs: Refs =
        serde_json::from_slice(&std::fs::read(data("reference_embeddings.json")).unwrap()).unwrap();
    let texts: Vec<String> = refs.text.keys().cloned().collect();
    let got = clip.texts(&texts).unwrap();
    for (t, e) in texts.iter().zip(&got) {
        let c = cos(e, &refs.text[t]);
        assert!(c > 0.999, "{t}: cos {c}");
    }
    let img = open_image(&data("gradient.png")).unwrap();
    let e = clip
        .images(std::slice::from_ref(&img), &mut |_| {})
        .unwrap();
    let c = cos(&e[0], &refs.image["gradient.png"]);
    assert!(c > 0.999, "image cos {c}");

    // CLIP semantics: the text closest to the picture of a red-blue gradient
    let t = clip
        .texts(&[
            "a gradient from red to blue".into(),
            "a photo of a cat".into(),
        ])
        .unwrap();
    assert!(dot(&e[0], &t[0]) > dot(&e[0], &t[1]));

    let l = bar_layout("make my top bar look like on a mac", &dirs)
        .unwrap()
        .unwrap();
    assert_eq!(l, layout::Preset::Mac.layout());
    let l = bar_layout("please improve the panel somehow", &dirs)
        .unwrap()
        .unwrap();
    assert_eq!(l, layout::Preset::Useful.layout());

    let hex = accent_for(
        &data("gradient.png"),
        &Prefs {
            accent_like: vec!["the color of the deep sea".into()],
            ..Prefs::default()
        },
        &dirs,
    )
    .unwrap();
    assert!(Lab::from_hex(&hex).unwrap().b < 0.0, "{hex}");

    let spot = clock_spot(&data("gradient.png"), (1920, 1080), (260, 260), &dirs).unwrap();
    assert!(
        spot.0 >= 0.0 && spot.0 <= 1660.0 && spot.1 >= 100.0 && spot.1 <= 820.0,
        "{spot:?}"
    );
}

#[test]
#[ignore = "live wallhaven + real model"]
fn live_pick() {
    let (_keep, cache) = cache();
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs {
        state: tmp.path().join("state"),
        cache,
        downloads: tmp.path().join("dl"),
    };
    let prefs = Prefs {
        candidates: 12,
        ..Prefs::default()
    };
    let stages = std::sync::Mutex::new(Vec::new());
    let t = std::time::Instant::now();
    let p = pick(
        &prefs,
        &dirs,
        &|s| stages.lock().unwrap().push(s),
        &Cancel::new(),
    )
    .unwrap();
    eprintln!("{p:?} in {:?}", t.elapsed());
    assert!(p.path.exists());
    let s = stages.into_inner().unwrap();
    assert_eq!(s.first(), Some(&Stage::Model));
    assert!(s.contains(&Stage::Download) && s.contains(&Stage::Accent));
    vote(&dirs, Vote::Like).unwrap();
}
