//! token-from-key-venv.sh (google-auth in the venv named by RUSTY_IMPULSE_VIRTUAL_ENV)
//! against the Rust job, with `token_uri` pointing at a local fake token endpoint.
use super::super::testutil::{self, Resp, Server};
use super::*;

fn venv() -> Option<String> {
    let v = std::env::var("RUSTY_IMPULSE_VIRTUAL_ENV").unwrap_or_default();
    let ok = !v.is_empty()
        && std::process::Command::new(format!("{v}/bin/python"))
            .args([
                "-c",
                "import google.auth, google.oauth2.service_account, requests",
            ])
            .output()
            .is_ok_and(|o| o.status.success());
    if !ok {
        crate::testenv::unavailable("RUSTY_IMPULSE_VIRTUAL_ENV has no google-auth venv");
    }
    ok.then_some(v)
}

fn rsa_pem(pkcs1: bool) -> Option<String> {
    if !testutil::have("openssl") {
        return None;
    }
    let o = std::process::Command::new("openssl")
        .args([
            "genpkey",
            "-algorithm",
            "RSA",
            "-pkeyopt",
            "rsa_keygen_bits:2048",
        ])
        .output()
        .ok()?;
    let pem = String::from_utf8(o.stdout).ok()?;
    if !pkcs1 {
        return Some(pem);
    }
    let mut c = std::process::Command::new("openssl")
        .args(["rsa", "-traditional"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    use std::io::Write;
    c.stdin.take()?.write_all(pem.as_bytes()).ok()?;
    String::from_utf8(c.wait_with_output().ok()?.stdout).ok()
}

fn key(srv: &Server, pem: &str, extra: &[(&str, J)]) -> String {
    let mut m = serde_json::Map::new();
    m.insert("type".into(), "service_account".into());
    m.insert("project_id".into(), "proj-1".into());
    m.insert("private_key_id".into(), "abc123".into());
    m.insert("private_key".into(), pem.into());
    m.insert(
        "client_email".into(),
        "sa@proj-1.iam.gserviceaccount.com".into(),
    );
    m.insert("token_uri".into(), format!("{}/token", srv.base()).into());
    m.insert("universe_domain".into(), "googleapis.com".into());
    for (k, v) in extra {
        if v.is_null() {
            m.remove(*k);
        } else {
            m.insert(k.to_string(), v.clone());
        }
    }
    J::Object(m).to_string()
}

struct Run {
    out: String,
    code: i32,
    t0: f64,
    t1: f64,
    reqs: Vec<testutil::Req>,
}

fn original(venv: &str, key: &str, srv: &Server) -> Run {
    let script = testutil::unshipped().join("services/gCloud/token-from-key-venv.sh");
    let t0 = now();
    let (out, err, code) = testutil::run(
        std::process::Command::new("bash")
            .arg("-c")
            .arg(format!("{} \"$SERVICE_KEY_CONTENT\"", script.display()))
            .env("SERVICE_KEY_CONTENT", key)
            .env("RUSTY_IMPULSE_VIRTUAL_ENV", venv),
    );
    elog::eprintln!("original stderr: {err}");
    Run {
        out,
        code,
        t0,
        t1: now(),
        reqs: srv.take(),
    }
}

fn rust(key: &str, srv: &Server) -> Run {
    let t0 = now();
    let (out, err, code) = testutil::capture(|o, e| run(key, &crate::svc::Stop::default(), o, e));
    elog::eprintln!("rust stderr: {err}");
    Run {
        out,
        code,
        t0,
        t1: now(),
        reqs: srv.take(),
    }
}

fn unb64(s: &str) -> Vec<u8> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(s)
        .unwrap()
}

/// The assertion of each request: header text, payload with iat/exp checked and removed.
fn assertion_shape(r: &testutil::Req, pem: &str) -> (String, String, String) {
    let body = String::from_utf8(r.body.clone()).unwrap();
    assert_eq!(r.method, "POST");
    assert_eq!(
        r.header("content-type"),
        Some("application/x-www-form-urlencoded")
    );
    let (a, g) = body.split_once('&').unwrap();
    assert_eq!(g, format!("grant_type={GRANT}"));
    let jwt = a.strip_prefix("assertion=").unwrap();
    let parts: Vec<&str> = jwt.split('.').collect();
    let header = String::from_utf8(unb64(parts[0])).unwrap();
    let payload = String::from_utf8(unb64(parts[1])).unwrap();
    let p: J = serde_json::from_str(&payload).unwrap();
    let (iat, exp) = (p["iat"].as_i64().unwrap(), p["exp"].as_i64().unwrap());
    assert_eq!(exp - iat, 3600);
    // PKCS#1 v1.5 signatures are deterministic: same input, same key, same bytes.
    let k = super::rsa_key(pem).unwrap();
    let input = format!("{}.{}", parts[0], parts[1]);
    let sig = super::b64(&super::rs256(&k, input.as_bytes()).unwrap());
    assert_eq!(sig, parts[2], "signature");
    let norm = payload
        .replacen(&iat.to_string(), "IAT", 1)
        .replacen(&exp.to_string(), "EXP", 1);
    (header, norm, parts[2].len().to_string())
}

fn check(case: &str, venv: &str, key: &str, pem: &str, srv: &Server) {
    let a = original(venv, key, srv);
    let b = rust(key, srv);
    // The script's wrapper exits 0 whatever python did; the job's code says whether a token
    // came out.
    assert_eq!(a.code, 0, "{case}");
    assert_eq!(
        b.code,
        if a.out.is_empty() { 1 } else { 0 },
        "{case}: exit code; stdout {:?} vs {:?}",
        a.out,
        b.out
    );
    assert_eq!(a.reqs.len(), b.reqs.len(), "{case}: request count");
    for (x, y) in a.reqs.iter().zip(&b.reqs) {
        assert_eq!(x.target, y.target, "{case}");
        assert_eq!(assertion_shape(x, pem), assertion_shape(y, pem), "{case}");
    }
    if a.out.is_empty() || b.out.is_empty() {
        assert_eq!(a.out, b.out, "{case}: stdout");
        return;
    }
    let (ja, jb): (J, J) = (
        serde_json::from_str(&a.out).unwrap(),
        serde_json::from_str(&b.out).unwrap(),
    );
    assert_eq!(ja["token"], jb["token"], "{case}");
    // Same text once the expiry (now + expires_in at each run) is lined up.
    let ea = ja["expiry"].as_i64().unwrap();
    let eb = jb["expiry"].as_i64().unwrap();
    assert_eq!(
        a.out.replace(&ea.to_string(), "E"),
        b.out.replace(&eb.to_string(), "E"),
        "{case}"
    );
    let secs = (ea as f64 - a.t0, eb as f64 - b.t0);
    let span = (a.t1 - a.t0 + 1.0, b.t1 - b.t0 + 1.0);
    assert!(
        (secs.0 - secs.1).abs() <= span.0.max(span.1),
        "{case}: expiry offsets {secs:?}"
    );
}

fn token_server(f: impl Fn(usize) -> Resp + Send + Sync + 'static) -> Server {
    let n = std::sync::atomic::AtomicUsize::new(0);
    testutil::serve(move |r| {
        if r.target != "/token" {
            return Resp::new(404, "no");
        }
        f(n.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
    })
}

fn json(status: u16, body: &str) -> Resp {
    let mut r = Resp::new(status, body);
    r.headers
        .push(("Content-Type".into(), "application/json".into()));
    r
}

#[test]
fn gcloud_token_equivalent() {
    let (Some(venv), Some(pem)) = (venv(), rsa_pem(false)) else {
        return;
    };
    let ok = |b: &'static str| move |_| json(200, b);
    type Case = (&'static str, Server, Vec<(&'static str, J)>);
    let cases: Vec<Case> = vec![
        (
            "ok",
            token_server(ok(
                r#"{"access_token":"ya29.tök\"en","expires_in":3599,"token_type":"Bearer"}"#,
            )),
            vec![],
        ),
        (
            "string expires_in, no kid",
            token_server(ok(r#"{"access_token":"t2","expires_in":"3600"}"#)),
            vec![("private_key_id", J::Null)],
        ),
        (
            "numeric kid and email",
            token_server(ok(r#"{"access_token":"t3","expires_in":10.5}"#)),
            vec![("private_key_id", 7.into()), ("client_email", "ü@x".into())],
        ),
        (
            "no expires_in",
            token_server(ok(r#"{"access_token":"t4"}"#)),
            vec![],
        ),
        (
            "no access_token",
            token_server(ok(r#"{"expires_in":5}"#)),
            vec![],
        ),
        ("not json", token_server(ok("hello")), vec![]),
        (
            "invalid_grant",
            token_server(|_| {
                json(
                    400,
                    r#"{"error":"invalid_grant","error_description":"bad"}"#,
                )
            }),
            vec![],
        ),
        (
            "retry 503 then ok",
            token_server(|n| {
                if n % 2 == 0 {
                    json(503, r#"{"error":"x"}"#)
                } else {
                    json(200, r#"{"access_token":"t5","expires_in":60}"#)
                }
            }),
            vec![],
        ),
        (
            "missing token_uri",
            token_server(ok("{}")),
            vec![("token_uri", J::Null)],
        ),
        (
            "missing private_key",
            token_server(ok("{}")),
            vec![("private_key", J::Null)],
        ),
        (
            "bad private_key",
            token_server(ok("{}")),
            vec![(
                "private_key",
                "-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----\n".into(),
            )],
        ),
    ];
    for (case, srv, extra) in &cases {
        let k = key(srv, &pem, extra);
        check(case, &venv, &k, &pem, srv);
    }
    let srv = token_server(ok("{}"));
    for k in ["", "not json", "[1]", "{\"client_email\":1}"] {
        check(k, &venv, k, &pem, &srv);
    }
}

#[test]
fn gcloud_token_pkcs1_key() {
    let (Some(venv), Some(pem)) = (venv(), rsa_pem(true)) else {
        return;
    };
    let srv = token_server(|_| json(200, r#"{"access_token":"p1","expires_in":3599}"#));
    check("pkcs1", &venv, &key(&srv, &pem, &[]), &pem, &srv);
}

/// Both sides with extra environment (trust-boundary switch, a dead HTTPS proxy so the
/// iamcredentials lookup fails offline): exit code, requests, and stdout with the token's
/// iat/exp and the expiry lined up.
fn check_env(case: &str, venv: &str, key: &str, pem: &str, srv: &Server, env: &[(&str, &str)]) {
    let script = testutil::unshipped().join("services/gCloud/token-from-key-venv.sh");
    let mut c = std::process::Command::new("bash");
    c.arg("-c")
        .arg(format!("{} \"$SERVICE_KEY_CONTENT\"", script.display()))
        .env("SERVICE_KEY_CONTENT", key)
        .env("RUSTY_IMPULSE_VIRTUAL_ENV", venv);
    for (k, v) in env {
        c.env(k, v);
    }
    let (out_a, err_a, code_a) = testutil::run(&mut c);
    let reqs_a = srv.take();
    let get = |k: &str| {
        env.iter()
            .find(|(n, _)| *n == k)
            .map(|(_, v)| v.to_string())
            .or_else(|| std::env::var(k).ok())
    };
    let (out_b, err_b, code_b) =
        testutil::capture(|o, e| run_env(key, &crate::svc::Stop::default(), &get, o, e));
    let reqs_b = srv.take();
    assert_eq!(
        (code_a, code_b),
        (0, if out_a.is_empty() { 1 } else { 0 }),
        "{case}: {err_a} / {err_b}"
    );
    assert_eq!(reqs_a.len(), reqs_b.len(), "{case}: requests");
    for (x, y) in reqs_a.iter().zip(&reqs_b) {
        assert_eq!(assertion_shape(x, pem), assertion_shape(y, pem), "{case}");
    }
    assert_eq!(
        out_a.is_empty(),
        out_b.is_empty(),
        "{case}: {out_a} / {out_b} / {err_a}"
    );
    if out_a.is_empty() {
        // Each side says why in its own words.
        assert!(!err_a.trim().is_empty(), "{case}: the script's stderr");
        assert!(
            err_b.starts_with("GoogleCloudToken: ") && err_b.trim_end().len() > 18,
            "{case}: {err_b:?}"
        );
        return;
    }
    let norm = |out: &str| -> (J, String) {
        let mut j: J = serde_json::from_str(out).unwrap();
        let tok = j["token"].as_str().unwrap().to_string();
        let parts: Vec<&str> = tok.split('.').collect();
        let mut shape = J::Null;
        if parts.len() == 3 && parts[0].starts_with("ey") {
            let header: J = serde_json::from_slice(&unb64(parts[0])).unwrap();
            let mut payload: J = serde_json::from_slice(&unb64(parts[1])).unwrap();
            let iat = payload["iat"].as_i64().unwrap();
            assert_eq!(payload["exp"].as_i64().unwrap() - iat, 3600, "{case}");
            assert!(
                (j["expiry"].as_i64().unwrap() - iat - 3600).abs() <= 1,
                "{case}"
            );
            let k = super::rsa_key(pem).unwrap();
            let input = format!("{}.{}", parts[0], parts[1]);
            let sig = super::b64(&super::rs256(&k, input.as_bytes()).unwrap());
            assert_eq!(sig, parts[2], "{case}: signature");
            payload["iat"] = 0.into();
            payload["exp"] = 0.into();
            let text = String::from_utf8(unb64(parts[1])).unwrap();
            shape = serde_json::json!([
                header,
                payload,
                String::from_utf8(unb64(parts[0])).unwrap(),
                text.len()
            ]);
            j["token"] = "T".into();
        }
        j["expiry"] = 0.into();
        (shape, j.to_string())
    };
    assert_eq!(norm(&out_a), norm(&out_b), "{case}");
}

#[test]
fn gcloud_token_other_universe_equivalent() {
    let (Some(venv), Some(pem)) = (venv(), rsa_pem(false)) else {
        return;
    };
    let srv = token_server(|_| json(200, r#"{"access_token":"u1","expires_in":3599}"#));
    for u in [
        J::String("example.com".into()),
        J::String("".into()),
        J::String("GOOGLEAPIS.COM".into()),
        5.into(),
        false.into(),
    ] {
        let k = key(&srv, &pem, &[("universe_domain", u.clone())]);
        check_env(&format!("universe {u}"), &venv, &k, &pem, &srv, &[]);
    }
    // `null` there means the key: `universe_domain: null` turns on the self-signed JWT too.
    let mut m: serde_json::Map<String, J> = serde_json::from_str(&key(&srv, &pem, &[])).unwrap();
    m.insert("universe_domain".into(), J::Null);
    check_env(
        "universe null",
        &venv,
        &J::Object(m).to_string(),
        &pem,
        &srv,
        &[],
    );
    let mut m: serde_json::Map<String, J> = serde_json::from_str(&key(&srv, &pem, &[])).unwrap();
    m.remove("private_key");
    m.insert("universe_domain".into(), "example.com".into());
    check_env("no key", &venv, &J::Object(m).to_string(), &pem, &srv, &[]);
}

/// google-auth 2.62 ignores the deprecated GOOGLE_AUTH_TRUST_BOUNDARY_ENABLED and the key's
/// `trust_boundary`: whatever they say, the same token and no `allowedLocations` lookup.
#[test]
fn gcloud_token_trust_boundary_ignored() {
    let (Some(venv), Some(pem)) = (venv(), rsa_pem(false)) else {
        return;
    };
    let srv = token_server(|_| json(200, r#"{"access_token":"t1","expires_in":3599}"#));
    let dead = [
        ("HTTPS_PROXY", "http://127.0.0.1:1"),
        ("https_proxy", "http://127.0.0.1:1"),
    ];
    let noop = serde_json::json!({"locations": [], "encodedLocations": "0x0"});
    let real = serde_json::json!({"locations": ["us-east1"], "encodedLocations": "0xA30"});
    type Case<'a> = (&'a str, &'a str, Vec<(&'a str, J)>);
    let cases: Vec<Case> = vec![
        ("off", "false", vec![]),
        ("zero", "0", vec![]),
        ("bogus", "maybe", vec![]),
        (
            "bogus other universe",
            "yes",
            vec![("universe_domain", "example.com".into())],
        ),
        (
            "on, other universe",
            "true",
            vec![("universe_domain", "example.com".into())],
        ),
        ("on, no-op cached", "TRUE", vec![("trust_boundary", noop)]),
        ("on, bad cached", "1", vec![("trust_boundary", "x".into())]),
        ("on, lookup fails", "true", vec![]),
        (
            "on, lookup fails with cache",
            "true",
            vec![("trust_boundary", real)],
        ),
    ];
    for (case, v, extra) in cases {
        let k = key(&srv, &pem, &extra);
        let mut env = dead.to_vec();
        env.push(("GOOGLE_AUTH_TRUST_BOUNDARY_ENABLED", v));
        check_env(case, &venv, &k, &pem, &srv, &env);
    }
}

/// `RUSTY_IMPULSE_VIRTUAL_ENV=… cargo test --features live -p qml --release gcloud_bench --
/// --nocapture --test-threads=1`
#[test]
#[cfg_attr(
    not(feature = "live"),
    ignore = "live: --features live, docs/benchmarks.md"
)]
fn gcloud_bench() {
    let (Some(venv), Some(pem)) = (venv(), rsa_pem(false)) else {
        return;
    };
    let srv = token_server(|_| json(200, r#"{"access_token":"b","expires_in":3599}"#));
    let k = key(&srv, &pem, &[]);
    let script = testutil::unshipped().join("services/gCloud/token-from-key-venv.sh");
    let n = 10;
    let (mut ow, mut oc, mut rw, mut rc) = (0.0, 0.0, 0.0, 0.0);
    for _ in 0..n {
        let (w, c) = testutil::measure_cmd(
            std::process::Command::new("bash")
                .arg("-c")
                .arg(format!("{} \"$SERVICE_KEY_CONTENT\"", script.display()))
                .env("SERVICE_KEY_CONTENT", &k)
                .env("RUSTY_IMPULSE_VIRTUAL_ENV", &venv),
        );
        ow += w;
        oc += c;
        let (w, c) = testutil::measure_thread(|| {
            testutil::capture(|o, e| run(&k, &crate::svc::Stop::default(), o, e));
        });
        rw += w;
        rc += c;
    }
    let n = n as f64;
    println!(
        "gcloud token: original wall {:.2} ms cpu {:.2} ms | rust wall {:.2} ms cpu(thread) {:.2} ms",
        ow / n * 1e3,
        oc / n * 1e3,
        rw / n * 1e3,
        rc / n * 1e3
    );
}

/// A stop during a retry's backoff ends the job at once: the backoff (here an hour) waits on
/// the stop itself.
#[test]
fn a_stop_during_the_backoff_ends_the_retries() {
    let stop = std::sync::Arc::new(crate::svc::Stop::default());
    let (sent_tx, sent) = std::sync::mpsc::channel();
    let s = stop.clone();
    std::thread::spawn(move || {
        sent.recv().unwrap();
        s.store(true, std::sync::atomic::Ordering::Release);
    });
    let sends = std::cell::Cell::new(0);
    let r = backoff_retries(&stop, 3600.0, || {
        sends.set(sends.get() + 1);
        let _ = sent_tx.send(());
        Ok(crate::http::Response {
            status: 503,
            reason: String::new(),
            headers: vec![],
            body: Box::new(std::io::Cursor::new(b"{}".to_vec())),
        })
    });
    assert_eq!(r, Err("interrupted".into()));
    assert_eq!(sends.get(), 1);
    assert_eq!(stop.wakeups(), 0);
}
