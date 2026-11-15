//! `scripts/ai/gemini-categorize-wallpaper.sh <image> [model] [prompt]`. Its caller is
//! `scripts/colors/switchwall.sh` (`categorize_wallpaper`, backgrounded), not a QML
//! `Process`, so this is a job for a native switchwall to start; nothing dispatches it yet.
use super::gemini::{self, Env};
use super::jq;
use crate::jobs::{Job, Pipes};
use base64::Engine;
use std::path::Path;

/// `basename "$p"`.
fn basename(p: &str) -> String {
    let t = p.trim_end_matches('/');
    if t.is_empty() {
        return if p.is_empty() {
            String::new()
        } else {
            "/".into()
        };
    }
    t.rsplit('/').next().unwrap_or(t).to_string()
}

/// `magick "$src" -resize 200x -quality 50 "$dst"`, kept in memory: the original's fixed
/// `/tmp/impulse-shell/ai/wallpaper.jpg` can be another user's symlink or swapped image.
pub fn resize(src: &str) -> Result<Vec<u8>, String> {
    let data =
        crate::osutil::read_regular(Path::new(src), imgtools::limit::MAX_DECODED_BYTES as u64)
            .map_err(|e| format!("magick: unable to open image `{src}': {e}"))?;
    let img = imgtools::limit::image(&data)
        .map_err(|e| format!("magick: no decode delegate for this image format `{src}': {e}"))?;
    let (w, h) = (img.width().max(1), img.height());
    let nh = ((h as f64 * 200.0 / w as f64) + 0.5).floor().max(1.0) as u32;
    let small = img
        .resize_exact(200, nh, image::imageops::FilterType::Lanczos3)
        .to_rgb8();
    let mut jpg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 50)
        .encode_image(&small)
        .map_err(|e| e.to_string())?;
    Ok(jpg)
}

pub fn run(
    script: &str,
    args: &[String],
    env: Env,
    base: &str,
    out: &mut dyn FnMut(String),
    err: &mut dyn FnMut(String),
) -> i32 {
    let src = args.first().cloned().unwrap_or_default();
    if src.is_empty() {
        out(format!(
            "Usage: {script} <image_path> [model] [prompt]\nTip: set GEMINI_WALLPAPER_MODEL and/or GEMINI_WALLPAPER_PROMPT to provide defaults.\n"
        ));
        return 1;
    }
    let arg = |i: usize| args.get(i).filter(|a| !a.is_empty()).cloned();
    let model =
        arg(1).unwrap_or_else(|| gemini::var_or(env, "GEMINI_WALLPAPER_MODEL", "gemini-2.5-flash"));
    let name = basename(&src);
    let prompt = arg(2).unwrap_or_else(|| {
        gemini::var_or(
            env,
            "GEMINI_WALLPAPER_PROMPT",
            &format!("Categorize the wallpaper. Its file name is {name}"),
        )
    });
    let jpg = resize(&src).map_err(|e| err(format!("{e}\n")));
    let key = gemini::api_key(err);
    let data = jpg
        .map(|b| base64::engine::general_purpose::STANDARD.encode(b))
        .unwrap_or_default();
    let payload = serde_json::json!({
        "contents": [{
            "parts": [
                { "inline_data": { "mime_type": "image/jpeg", "data": data } },
                { "text": prompt },
            ],
        }],
        "generationConfig": {
            "responseMimeType": "text/x.enum",
            "responseSchema": {
                "type": "string",
                "enum": [
                    "abstract", "anime", "city", "minimalist", "landscape", "plants", "person",
                    "space",
                ],
            },
            "temperature": 0,
        },
    })
    .to_string();
    let response = gemini::generate(base, &model, &key, &payload);
    let (text, jerr, code) = jq::gemini_text(&format!("{response}\n"));
    out(text);
    err(jerr);
    code
}

/// `"$SCRIPT_DIR/../ai/gemini-categorize-wallpaper.sh" "$img"` as a job.
pub fn job(script: String, args: Vec<String>) -> Job {
    Box::new(move |p: &Pipes, _stop| {
        run(
            &script,
            &args,
            &|k| std::env::var(k).ok(),
            gemini::ENDPOINT,
            &mut |s| p.out(s),
            &mut |s| p.err(s),
        )
    })
}

/// `img_cat=$("$SCRIPT_DIR/../ai/gemini-categorize-wallpaper.sh" "$1")` in switchwall: stdout
/// only, stderr goes to switchwall's (discarded) stderr.
pub fn for_switchwall(script: &Path, img: &str) -> Option<String> {
    Some(capture(
        script,
        img,
        &|k| std::env::var(k).ok(),
        gemini::ENDPOINT,
    ))
}

/// `$(script "$img")`: the stdout `for_switchwall` hands to switchwall.
pub fn capture(script: &Path, img: &str, env: Env, base: &str) -> String {
    let mut out = String::new();
    run(
        &script.to_string_lossy(),
        &[img.to_string()],
        env,
        base,
        &mut |s| out.push_str(&s),
        &mut |_| {},
    );
    out
}
