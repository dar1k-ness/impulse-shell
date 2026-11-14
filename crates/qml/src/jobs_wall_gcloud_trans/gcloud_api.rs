//! The Google Cloud requests of modules/common/models/gCloud:
//! - `Impulse/GoogleTranslate` (GCloudTranslate.qml): POST the strings to `…:translateText`;
//! - `Impulse/GoogleVision` (GCloudVision.qml): the image is read and base64-encoded in
//!   memory and posted to `images:annotate`. No temp files, so two screen translations can't
//!   clobber each other's payload and nothing multi-megabyte is left in /tmp.
//!
//! A request that fails before Google answers (no connection, an image that cannot be read)
//! prints a Google-style `{"error": {…}}`, so `handleApiOutput` reports the real cause.
//! Requests give up on a stalled connection ([`crate::http::STALL`]) instead of leaving the UI
//! processing forever.
use crate::jobs::Job;
use crate::runtime::World;
use crate::value::Value;
use base64::Engine;

/// Environment lookup for the HTTP proxy variables curl reads.
pub(super) type Env<'a> = &'a dyn Fn(&str) -> Option<String>;

fn sys_env(k: &str) -> Option<String> {
    std::env::var(k).ok()
}

const TRANSLATE: &str = "https://translation.googleapis.com/v3/projects/";
const VISION: &str = "https://vision.googleapis.com/v1/images:annotate";

pub(super) type Image = Result<String, String>;

pub static VISION_ELEMENT: crate::impulse::Element = crate::impulse::Element {
    job: Some(vision),
    ..crate::impulse::Element::new("GoogleVision")
};

/// `Impulse/GoogleVision`: the text Google Vision finds in the image file `image`
/// (`images:annotate`'s answer on stdout), as project `project` with access token `token`.
fn vision(w: &mut World, p: usize) -> Option<Job> {
    let token = crate::jobs::text(w, p, "token");
    let project = crate::jobs::text(w, p, "project");
    let image = crate::jobs::text(w, p, "image");
    Some(Box::new(move |io, _| {
        let img = encode_image(&image);
        if let Err(e) = &img {
            io.err(format!("{e}\n"));
        }
        let (out, code) = annotate(VISION, &token, &project, &img, &sys_env);
        io.out(out);
        code
    }))
}

pub static TRANSLATE_ELEMENT: crate::impulse::Element = crate::impulse::Element {
    job: Some(translate),
    ..crate::impulse::Element::new("GoogleTranslate")
};

/// `Impulse/GoogleTranslate`: `strings` translated to `language` by Google Translate
/// (`translateText`'s answer on stdout), as project `project` with access token `token`.
fn translate(w: &mut World, p: usize) -> Option<Job> {
    let token = crate::jobs::text(w, p, "token");
    let project = crate::jobs::text(w, p, "project");
    let lang = crate::jobs::text(w, p, "language");
    let strings: Vec<String> = match w.read(p, "strings") {
        Some(Value::Obj(o)) => o
            .borrow()
            .array
            .iter()
            .flatten()
            .map(|v| v.to_js_string())
            .collect(),
        _ => vec![],
    };
    let body = translate_body(&lang, &strings);
    let url = format!("{TRANSLATE}{project}:translateText");
    Some(Box::new(move |io, _| {
        let (out, code) = translate_post(&url, &token, &project, &body, &sys_env);
        io.out(out);
        code
    }))
}

fn headers(token: &str, project: &str) -> Vec<(String, String)> {
    vec![
        ("Authorization".to_string(), format!("Bearer {token}")),
        ("x-goog-user-project".to_string(), project.to_string()),
        ("Content-Type".to_string(), "application/json".to_string()),
    ]
}

/// A Google API error body, as `handleApiOutput` reads it.
pub(super) fn error_json(message: &str) -> String {
    serde_json::json!({"error": {"code": 0, "message": message}}).to_string()
}

/// POST with the API headers: the response body (whatever the status, as curl prints it), or
/// an error body naming the transport failure plus curl's exit code for it.
///
/// With `follow` (curl `-L`), 307/308 repeat the POST with its body, 301/302/303 continue as a
/// bodyless GET, the bearer token only goes to the original host, and 50 hops is the limit.
/// ureq can't replay a POST body itself (it fails a 307), so the hops are taken here.
fn post(
    url: &str,
    token: &str,
    project: &str,
    body: &[u8],
    follow: bool,
    env: Env,
) -> (String, i32) {
    let o = crate::http::Opts {
        stall: true,
        ..Default::default()
    };
    let failed = |msg: String, code| (error_json(&msg), code);
    let first = url::Url::parse(url).map(|u| u.origin());
    let mut url = url.to_string();
    let mut method = "POST";
    for _ in 0..=50 {
        let mut h = headers(token, project);
        if url::Url::parse(&url).map(|u| u.origin()) != first {
            h.remove(0);
        }
        let data = (method == "POST").then_some(body);
        if data.is_none() {
            h.retain(|(k, _)| k != "Content-Type");
        }
        let r = match crate::http::send_env(method, &url, &h, data, o, env) {
            Ok(r) => r,
            Err(e) => {
                // curl 8 exits 7 for a tunnel the proxy refuses (7.x said 56).
                let code = match e {
                    ureq::Error::ConnectProxyFailed(_) => 7,
                    _ => crate::jobs_ai::curl_code(&e),
                };
                return failed(e.to_string(), code);
            }
        };
        let next = match (follow, r.status, r.header("location")) {
            (true, 301 | 302 | 303 | 307 | 308, Some(l)) => Some((r.status, resolve(&url, l))),
            _ => None,
        };
        let Some((status, next)) = next else {
            return match r.bytes() {
                Ok(b) => (String::from_utf8_lossy(&b).into_owned(), 0),
                Err(e) => {
                    let msg = e.to_string();
                    failed(msg, crate::jobs_ai::curl_code(&ureq::Error::Io(e)))
                }
            };
        };
        let _ = r.bytes();
        if !matches!(status, 307 | 308) {
            method = "GET";
        }
        url = next;
    }
    failed("Too many redirects".to_string(), 47)
}

/// A `Location` against the URL it came from (RFC 3986 reference resolution).
fn resolve(base: &str, loc: &str) -> String {
    match url::Url::parse(base).and_then(|b| b.join(loc)) {
        Ok(u) => u.into(),
        Err(_) => loc.to_string(),
    }
}

/// Translate's `curl -sL -X POST … -d '<body>'`.
pub(super) fn translate_post(
    url: &str,
    token: &str,
    project: &str,
    body: &str,
    env: Env,
) -> (String, i32) {
    post(url, token, project, body.as_bytes(), true, env)
}

/// `JSON.stringify({targetLanguageCode, contents, mimeType: "text/plain"})`.
pub(super) fn translate_body(lang: &str, strings: &[String]) -> String {
    let js = |s: &str| serde_json::to_string(s).unwrap_or_default();
    let contents: Vec<String> = strings.iter().map(|s| js(s)).collect();
    format!(
        "{{\"targetLanguageCode\":{},\"contents\":[{}],\"mimeType\":\"text/plain\"}}",
        js(lang),
        contents.join(",")
    )
}

/// The `images:annotate` request for one base64 image.
pub(super) fn vision_body(b64: &str) -> String {
    serde_json::json!({
        "requests": [{
            "image": {"content": b64},
            "features": [{"type": "DOCUMENT_TEXT_DETECTION"}],
        }]
    })
    .to_string()
}

/// Vision's `curl -s -X POST …/v1/images:annotate`; an unreadable image is reported without
/// uploading anything.
pub(super) fn annotate(
    url: &str,
    token: &str,
    project: &str,
    image: &Image,
    env: Env,
) -> (String, i32) {
    match image {
        Ok(b64) => post(url, token, project, vision_body(b64).as_bytes(), false, env),
        Err(e) => (error_json(e), 1),
    }
}

/// The image file as standard base64 (what `base64 -w 0` prints).
pub(super) fn encode_image(path: &str) -> Image {
    match std::fs::read(path) {
        Ok(d) => Ok(base64::engine::general_purpose::STANDARD.encode(d)),
        Err(e) => {
            let s = e.to_string();
            let s = s.find(" (os error ").map_or(s.as_str(), |i| &s[..i]);
            Err(format!("Cannot read image {path}: {s}"))
        }
    }
}

#[cfg(test)]
mod tests;
