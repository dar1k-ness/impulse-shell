//! The native GCloudTranslate / GCloudVision steps against the original's bash lines (curl, base64, jq)
//! where those are right, and against the intended result where the original's are not: no temp files,
//! an unreadable image or a failed connection reported instead of an unparsable empty reply.
use super::super::testutil::{self, Resp, Server};
use super::*;

fn esc(s: &str) -> String {
    s.replace('\'', "'\\''")
}

fn bash(script: &str) -> (String, String, i32) {
    testutil::run(std::process::Command::new("bash").arg("-c").arg(script))
}

type Shape = (String, String, Vec<u8>, Vec<(String, String)>);

/// What curl and the native client must agree on: method, target, body and the headers
/// except the client's own `User-Agent` / `Accept-Encoding`.
fn shape(r: &testutil::Req) -> Shape {
    let mut h: Vec<(String, String)> = r
        .headers
        .iter()
        .map(|(k, v)| (k.to_ascii_lowercase(), v.clone()))
        .filter(|(k, _)| k != "user-agent" && k != "accept-encoding")
        .collect();
    h.sort();
    (r.method.clone(), r.target.clone(), r.body.clone(), h)
}

fn shapes(srv: &Server) -> Vec<Shape> {
    srv.take().iter().map(shape).collect()
}

fn api_server() -> Server {
    testutil::serve(|r| match r.target.as_str() {
        "/keep" => {
            let mut x = Resp::new(307, "");
            x.headers
                .push(("Location".into(), "projects/p-1:translateText".into()));
            x
        }
        "/away" => {
            let port = r
                .header("host")
                .and_then(|h| h.rsplit(':').next())
                .unwrap_or("");
            let mut x = Resp::new(307, "");
            x.headers.push((
                "Location".into(),
                format!("http://localhost:{port}/v3/projects/p-1:translateText"),
            ));
            x
        }
        "/moved" => {
            let mut x = Resp::new(303, "");
            x.headers.push(("Location".into(), "/done".into()));
            x
        }
        "/loop" => {
            let mut x = Resp::new(307, "");
            x.headers.push(("Location".into(), "/loop".into()));
            x
        }
        "/denied" => Resp::new(403, "{\"error\": {\"code\": 403, \"message\": \"no\"}}"),
        _ => Resp::new(200, format!("{{\"echo\": {}}}", r.body.len())),
    })
}

/// `JSON.stringify({targetLanguageCode, contents, mimeType})` of [`strings`] in V4 (and node).
const STRINGIFIED: &str = "{\"targetLanguageCode\":\"ru_RU\",\"contents\":[\"Hello\",\"it's \\\"quoted\\\" \\\\ back\",\"line\\nbreak\\ttab\",\"Привет 你好 😀\",\"\\u0001\\u001f\u{7f}\",\"\"],\"mimeType\":\"text/plain\"}";

const TOKEN: &str = "ya29.a0AfB_byC-tok3n";
const PROJECT: &str = "p-1";

fn curl_translate(url: &str, body: &str) -> String {
    format!(
        "curl -sL -X POST -H \"Authorization: Bearer {TOKEN}\" -H \"x-goog-user-project: \
         {PROJECT}\" -H \"Content-Type: application/json\" -d '{}' \"{url}\"",
        esc(body)
    )
}

fn strings() -> Vec<String> {
    [
        "Hello",
        "it's \"quoted\" \\ back",
        "line\nbreak\ttab",
        "Привет 你好 😀",
        "\u{1}\u{1f}\u{7f}",
        "",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// The request and reply match the original's curl line byte for byte.
#[test]
fn gcloud_translate_equivalent() {
    if !testutil::have("curl") {
        return;
    }
    let srv = api_server();
    let body = translate_body("ru_RU", &strings());
    assert_eq!(body, STRINGIFIED, "JSON.stringify");
    for path in ["/v3/projects/p-1:translateText", "/denied", "/keep"] {
        let url = format!("{}{path}", srv.base());
        let (out_a, _, code_a) = bash(&curl_translate(&url, &body));
        let a = shapes(&srv);
        let (out_b, code_b) = translate_post(&url, TOKEN, PROJECT, &body, &sys_env);
        let b = shapes(&srv);
        assert_eq!((out_a, code_a), (out_b, code_b), "{path}");
        assert_eq!(a, b, "{path}");
    }
}

/// A redirect to another host does not carry the bearer token along.
#[test]
fn gcloud_translate_redirect_drops_token() {
    let srv = api_server();
    let body = translate_body("de", &strings());
    let (out, code) = translate_post(
        &format!("{}/away", srv.base()),
        TOKEN,
        PROJECT,
        &body,
        &sys_env,
    );
    let reqs = srv.take();
    assert_eq!((out, code), (format!("{{\"echo\": {}}}", body.len()), 0));
    assert_eq!(reqs.len(), 2);
    assert_eq!(reqs[1].target, "/v3/projects/p-1:translateText");
    assert_eq!(reqs[1].body, body.as_bytes());
    assert_eq!(reqs[1].header("authorization"), None);
}

/// A 303 continues as a GET without the JSON body; an endless redirect is an error, not "".
#[test]
fn gcloud_translate_redirects() {
    let srv = api_server();
    let (out, code) = translate_post(
        &format!("{}/moved", srv.base()),
        TOKEN,
        PROJECT,
        "{}",
        &sys_env,
    );
    let reqs = srv.take();
    assert_eq!((out.as_str(), code), ("{\"echo\": 0}", 0));
    assert_eq!(
        reqs.iter()
            .map(|r| (r.method.as_str(), r.target.as_str()))
            .collect::<Vec<_>>(),
        [("POST", "/moved"), ("GET", "/done")]
    );
    assert_eq!(reqs[1].header("authorization").map(|_| ()), Some(()));
    assert_eq!(reqs[1].header("content-type"), None);
    let (out, code) = translate_post(
        &format!("{}/loop", srv.base()),
        TOKEN,
        PROJECT,
        "{}",
        &sys_env,
    );
    assert_eq!(srv.take().len(), 51);
    assert_eq!((out, code), (error_json("Too many redirects"), 47));
}

/// curl prints nothing when it can't connect, which the original shows as "Failed to parse API
/// response"; the port prints an API error naming the failure (curl's exit code kept).
#[test]
fn gcloud_api_connection_failure_reported() {
    let dead = "http://127.0.0.1:1/v3/projects/p-1:translateText";
    for (out, code) in [
        translate_post(dead, TOKEN, PROJECT, "{}", &sys_env),
        annotate(dead, TOKEN, PROJECT, &Ok("AA==".into()), &sys_env),
    ] {
        assert_eq!(code, 7);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(!v["error"]["message"].as_str().unwrap().is_empty(), "{out}");
        assert!(v["error"]["code"].is_number());
    }
}

/// The original's request (`base64 -w 0` → `jq --rawfile` → `curl -d @payload`) and the in-memory one
/// carry the same JSON, headers and target; nothing is written to disk.
#[test]
fn gcloud_vision_equivalent() {
    if !testutil::have("curl") || !testutil::have("jq") || !testutil::have("base64") {
        elog::eprintln!("SKIP: no curl/jq/base64");
        return;
    }
    let srv = api_server();
    let root = testutil::tmpdir("gcv");
    let images: &[(&str, Vec<u8>)] = &[
        ("shot.png", (0..=255u8).cycle().take(3001).collect()),
        ("it's a shot.png", b"\x89PNG\r\n\x1a\n".to_vec()),
        ("empty.png", vec![]),
    ];
    for (name, bytes) in images {
        let image = root.join(name).display().to_string();
        std::fs::write(&image, bytes).unwrap();
        let tmp = root.join("tmp");
        let (b64, payload) = (tmp.join("b64"), tmp.join("payload.json"));
        let (b64, payload) = (b64.display(), payload.display());
        let script = format!(
            "mkdir -p '{}'; base64 '{}' -w 0 > '{b64}'; jq -n --rawfile content '{b64}' \
             '{{\"requests\": [{{\"image\": {{\"content\": $content}}, \"features\": \
             [{{\"type\": \"DOCUMENT_TEXT_DETECTION\"}}]}}]}}' > '{payload}'; curl -s -X POST \
             -H \"Authorization: Bearer {TOKEN}\" -H \"x-goog-user-project: {PROJECT}\" -H \
             \"Content-Type: application/json\" {}/v1/images:annotate -d @'{payload}'",
            tmp.display(),
            esc(&image),
            srv.base()
        );
        let (out_a, _, code_a) = bash(&script);
        let _ = std::fs::remove_dir_all(&tmp);
        let a = shapes(&srv);
        let before = std::fs::read_dir(&root).unwrap().count();
        let img = encode_image(&image);
        let url = format!("{}/v1/images:annotate", srv.base());
        let (out_b, code_b) = annotate(&url, TOKEN, PROJECT, &img, &sys_env);
        let b = shapes(&srv);
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), before, "{name}");
        let json = |s: &Shape| serde_json::from_slice::<serde_json::Value>(&s.2).unwrap();
        let heads = |s: &Shape| {
            let mut s = s.clone();
            s.2.clear();
            s.3.retain(|(k, _)| k != "content-length");
            s
        };
        assert_eq!((a.len(), b.len()), (1, 1), "{name}");
        assert_eq!(json(&a[0]), json(&b[0]), "{name}");
        assert_eq!(heads(&a[0]), heads(&b[0]), "{name}");
        let len = b[0].2.len();
        assert_eq!(
            (out_b, code_b),
            (format!("{{\"echo\": {len}}}"), 0),
            "{name}"
        );
        assert_eq!(code_a, 0, "{name} {out_a}");
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// The original uploads an empty image when base64 can't read the file and shows "Failed to parse API
/// response" or Google's complaint; the port names the file and uploads nothing.
#[test]
fn gcloud_vision_unreadable_image_reported() {
    let srv = api_server();
    let root = testutil::tmpdir("gcv-bad");
    std::fs::create_dir(root.join("dir.png")).unwrap();
    let url = format!("{}/v1/images:annotate", srv.base());
    for name in ["none.png", "dir.png"] {
        let path = root.join(name).display().to_string();
        let img = encode_image(&path);
        let (out, code) = annotate(&url, TOKEN, PROJECT, &img, &sys_env);
        assert_eq!(code, 1);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let msg = v["error"]["message"].as_str().unwrap();
        assert!(
            msg.starts_with(&format!("Cannot read image {path}: ")),
            "{msg}"
        );
    }
    assert!(srv.take().is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

/// The real Google URLs with the mock as `https_proxy` (curl's environment and the port's
/// curl-rule proxy lookup): both open the same CONNECT tunnel; a proxy that refuses it is a
/// failed request on both sides (exit 7 as curl 8 has it; curl 7.x said 56).
#[test]
fn gcloud_api_through_proxy_equivalent() {
    if !testutil::have("curl") {
        return;
    }
    let srv = testutil::serve(|_| Resp::new(403, "denied"));
    let proxy = srv.base();
    let env = |k: &str| (k == "https_proxy").then(|| proxy.clone());
    let pre = format!(
        "unset no_proxy NO_PROXY all_proxy ALL_PROXY HTTPS_PROXY; export https_proxy={proxy}; "
    );
    let tunnel = |srv: &Server| -> Vec<(String, String)> {
        srv.take()
            .into_iter()
            .map(|r| (r.method, r.target))
            .collect()
    };
    let body = translate_body("de", &["it's".to_string(), "Привет".to_string()]);
    let url = format!("{TRANSLATE}{PROJECT}:translateText");
    let (out_a, _, code_a) = bash(&format!("{pre}{}", curl_translate(&url, &body)));
    let a = tunnel(&srv);
    let (out_b, code_b) = translate_post(&url, TOKEN, PROJECT, &body, &env);
    let b = tunnel(&srv);
    assert_eq!(out_a, "", "translate");
    assert!(matches!(code_a, 7 | 56), "translate: curl {code_a}");
    assert_eq!(code_b, 7, "translate: {out_b}");
    assert!(out_b.contains("\"error\""), "{out_b}");
    assert_eq!(a, b, "translate");
    assert_eq!(
        a,
        [(
            "CONNECT".to_string(),
            "translation.googleapis.com:443".to_string()
        )]
    );

    let script = format!(
        "{pre}curl -s -X POST -H \"Authorization: Bearer {TOKEN}\" -H \"x-goog-user-project: \
         {PROJECT}\" -H \"Content-Type: application/json\" {VISION} -d '{}'",
        vision_body("AA==")
    );
    let (out_a, _, code_a) = bash(&script);
    let a = tunnel(&srv);
    let (out_b, code_b) = annotate(VISION, TOKEN, PROJECT, &Ok("AA==".into()), &env);
    let b = tunnel(&srv);
    assert_eq!(out_a, "", "vision");
    assert!(matches!(code_a, 7 | 56), "vision: curl {code_a}");
    assert_eq!(code_b, 7, "vision: {out_b}");
    assert!(out_b.contains("\"error\""), "{out_b}");
    assert_eq!(a, b, "vision");
    assert_eq!(
        a,
        [(
            "CONNECT".to_string(),
            "vision.googleapis.com:443".to_string()
        )]
    );
}
