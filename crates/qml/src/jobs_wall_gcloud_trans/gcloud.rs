//! services/GoogleCloud.qml `tokenProc`: `token-from-key-venv.sh "$SERVICE_KEY_CONTENT"`, i.e.
//! token_from_key.py on current google-auth (2.62): a service-account JWT grant (RS256
//! assertion, scope cloud-platform), or the self-signed scoped JWT it uses outside the default
//! universe; printed as `{"token": ..., "expiry": ...}`. The deprecated trust-boundary lookup
//! (GOOGLE_AUTH_TRUST_BOUNDARY_ENABLED, 2.49) is gone there and here.
use super::py_dumps;
use crate::jobs::Job;
use crate::runtime::World;
use base64::Engine;
use serde_json::Value as J;

const SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";
/// google-auth `_GOOGLE_OAUTH2_TOKEN_ENDPOINT`: the assertion's `aud`, whatever `token_uri` is.
const AUD: &str = "https://oauth2.googleapis.com/token";
const DEFAULT_UNIVERSE: &str = "googleapis.com";
const GRANT: &str = "urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer";

pub static TOKEN: crate::impulse::Element = crate::impulse::Element {
    job: Some(token),
    ..crate::impulse::Element::new("GoogleCloudToken")
};

/// `Impulse/GoogleCloudToken`: an access token for the service account whose key file is
/// the JSON text `key`, printed as `{"token": …, "expiry": <seconds>}`.
fn token(w: &mut World, p: usize) -> Option<Job> {
    let key = crate::jobs::text(w, p, "key");
    Some(Box::new(move |io, stop| {
        run(&key, stop, &|s| io.out(s), &|s| io.err(s))
    }))
}

fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn b64(b: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

/// The signed assertion google-auth's `jwt.encode` builds (same JSON text, so the same bytes).
pub(super) fn assertion(info: &serde_json::Map<String, J>, iat: i64) -> Result<String, String> {
    let mut payload = serde_json::Map::new();
    payload.insert("iat".into(), iat.into());
    payload.insert("exp".into(), (iat + 3600).into());
    payload.insert("iss".into(), info["client_email"].clone());
    payload.insert("aud".into(), AUD.into());
    payload.insert("scope".into(), SCOPE.into());
    sign(info, payload)
}

/// `jwt.Credentials._make_jwt` for `with_scopes` outside the default universe: the token
/// itself, issued and signed by the service account (no request).
fn self_signed(info: &serde_json::Map<String, J>, iat: i64) -> Result<String, String> {
    let mut payload = serde_json::Map::new();
    payload.insert("iss".into(), info["client_email"].clone());
    payload.insert("sub".into(), info["client_email"].clone());
    payload.insert("iat".into(), iat.into());
    payload.insert("exp".into(), (iat + 3600).into());
    payload.insert("scope".into(), SCOPE.into());
    sign(info, payload)
}

/// `jwt.encode(signer, payload)` with the key `_service_account_info.from_dict` loads.
fn sign(
    info: &serde_json::Map<String, J>,
    payload: serde_json::Map<String, J>,
) -> Result<String, String> {
    let pem = match info.get("private_key") {
        Some(J::String(s)) => s,
        Some(_) => return Err("Could not deserialize key data.".into()),
        None => {
            return Err("The private_key field was not found in the service account info.".into());
        }
    };
    let key = rsa_key(pem).ok_or_else(|| "Could not deserialize key data.".to_string())?;
    let mut header = serde_json::Map::new();
    header.insert("typ".into(), "JWT".into());
    header.insert("alg".into(), "RS256".into());
    match info.get("private_key_id") {
        None | Some(J::Null) => {}
        Some(k) => {
            header.insert("kid".into(), k.clone());
        }
    }
    let input = format!(
        "{}.{}",
        b64(py_dumps(&J::Object(header)).as_bytes()),
        b64(py_dumps(&J::Object(payload)).as_bytes())
    );
    let sig = rs256(&key, input.as_bytes()).map_err(|e| e.to_string())?;
    Ok(format!("{input}.{}", b64(&sig)))
}

/// The RSA key of a PEM `PRIVATE KEY` (PKCS#8) or `RSA PRIVATE KEY` (PKCS#1) block, the two
/// forms `serialization.load_pem_private_key` takes for a service account's key.
fn rsa_key(pem: &str) -> Option<ring::rsa::KeyPair> {
    use rustls_pki_types::{PrivateKeyDer, pem::PemObject};
    match PrivateKeyDer::from_pem_slice(pem.as_bytes()).ok()? {
        PrivateKeyDer::Pkcs8(k) => ring::rsa::KeyPair::from_pkcs8(k.secret_pkcs8_der()).ok(),
        PrivateKeyDer::Pkcs1(k) => ring::rsa::KeyPair::from_der(k.secret_pkcs1_der()).ok(),
        _ => None,
    }
}

/// RSASSA-PKCS1-v1_5 with SHA-256 (JWS `RS256`, RFC 7518 3.3).
fn rs256(key: &ring::rsa::KeyPair, msg: &[u8]) -> Result<Vec<u8>, ring::error::Unspecified> {
    let mut sig = vec![0; key.public().modulus_len()];
    key.sign(
        &ring::signature::RSA_PKCS1_SHA256,
        &ring::rand::SystemRandom::new(),
        msg,
        &mut sig,
    )?;
    Ok(sig)
}

/// `transport.DEFAULT_RETRYABLE_STATUS_CODES` and the RFC 6749 error codes `_can_retry` accepts.
fn retryable(status: u16, data: &J) -> bool {
    if matches!(status, 500 | 503 | 504 | 408 | 429) {
        return true;
    }
    let (Some(desc), Some(code)) = (
        data.get("error_description").map_or(Some(""), J::as_str),
        data.get("error").map_or(Some(""), J::as_str),
    ) else {
        return false;
    };
    [code, desc].iter().any(|e| {
        matches!(
            *e,
            "internal_failure" | "server_error" | "temporarily_unavailable"
        )
    })
}

fn uniform(lo: f64, hi: f64) -> f64 {
    let mut b = [0u8; 8];
    // SAFETY: getrandom(2) writes at most 8 bytes to `b`, a live 8-byte buffer.
    unsafe { libc::getrandom(b.as_mut_ptr().cast(), 8, 0) };
    lo + (hi - lo) * (u64::from_ne_bytes(b) >> 11) as f64 / (1u64 << 53) as f64
}

/// The token for the service-account `key` as a JSON line on `out`, code 0; the reason on
/// `err` and code 1 when there is none.
pub(super) fn run(
    key: &str,
    stop: &crate::svc::Stop,
    out: &dyn Fn(String),
    err: &dyn Fn(String),
) -> i32 {
    run_env(key, stop, &|k| std::env::var(k).ok(), out, err)
}

/// [`run`] with the environment google-auth and requests read (proxies).
pub(super) fn run_env(
    key: &str,
    stop: &crate::svc::Stop,
    env: Env,
    out: &dyn Fn(String),
    err: &dyn Fn(String),
) -> i32 {
    match token_json(key, stop, env) {
        Ok(s) => {
            out(s + "\n");
            0
        }
        Err(e) => {
            err(format!("GoogleCloudToken: {e}\n"));
            1
        }
    }
}

type Env<'a> = &'a dyn Fn(&str) -> Option<String>;

fn token_json(key: &str, stop: &crate::svc::Stop, env: Env) -> Result<String, String> {
    let info = match serde_json::from_str::<J>(key).map_err(|e| e.to_string())? {
        J::Object(m) => m,
        _ => return Err("the key is not a JSON object".into()),
    };
    let missing: Vec<&str> = ["client_email", "token_uri"]
        .into_iter()
        .filter(|k| !info.contains_key(*k))
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "Service account info was not in the expected format, missing fields {}.",
            missing.join(", ")
        ));
    }
    // `universe_domain != DEFAULT` turns on `_always_use_jwt_access`; `_universe_domain` is
    // the value or, when falsy, the default.
    let raw = info.get("universe_domain");
    let (token, expiry) = if raw.is_none_or(|u| u.as_str() == Some(DEFAULT_UNIVERSE)) {
        grant(&info, stop, env)?
    } else {
        let t = now();
        let jwt = self_signed(&info, t.floor() as i64)?;
        (J::String(jwt), expiry_after(t, 3600.0))
    };
    let mut o = serde_json::Map::new();
    o.insert("token".into(), token);
    o.insert("expiry".into(), expiry.into());
    Ok(py_dumps(&J::Object(o)))
}

/// `utcnow() + timedelta(seconds=…)` (microsecond resolution), then whole seconds.
fn expiry_after(now: f64, secs: f64) -> i64 {
    (((now * 1e6).floor() + (secs * 1e6).round()) / 1e6).floor() as i64
}

/// `jwt_grant` against `token_uri`: the access token and its expiry.
fn grant(
    info: &serde_json::Map<String, J>,
    stop: &crate::svc::Stop,
    env: Env,
) -> Result<(J, i64), String> {
    let uri = match &info["token_uri"] {
        J::String(s) => s.clone(),
        v => v.to_string(),
    };
    let body = format!(
        "assertion={}&grant_type={GRANT}",
        assertion(info, now() as i64)?
    );
    let headers = vec![(
        "Content-Type".to_string(),
        "application/x-www-form-urlencoded".to_string(),
    )];
    let (status, data) = with_retries(stop, || {
        crate::http::send_env("POST", &uri, &headers, Some(body.as_bytes()), follow(), env)
    })?;
    if status == 200 {
        return grant_output(&data);
    }
    Err(error_details(&data))
}

fn follow() -> crate::http::Opts {
    crate::http::Opts {
        follow: true,
        ..Default::default()
    }
}

/// `_exponential_backoff.ExponentialBackoff()` around one request, as `_token_endpoint_request`
/// and `_lookup_trust_boundary_request` loop: the last status and decoded body.
fn with_retries(
    stop: &crate::svc::Stop,
    send: impl Fn() -> Result<crate::http::Response, crate::http::Error>,
) -> Result<(u16, J), String> {
    backoff_retries(stop, 1.0, send)
}

/// [`with_retries`] with a first backoff of about `wait` seconds, doubling after it. A backoff
/// waits on `stop` itself, so a stop during one ends the job at once.
fn backoff_retries(
    stop: &crate::svc::Stop,
    mut wait: f64,
    send: impl Fn() -> Result<crate::http::Response, crate::http::Error>,
) -> Result<(u16, J), String> {
    let mut last = (0, J::Null);
    for attempt in 0..3 {
        if attempt > 0 {
            let d = std::time::Duration::from_secs_f64(uniform(wait * 0.9, wait * 1.1));
            if stop.wait_timeout(d) {
                return Err("interrupted".into());
            }
            wait *= 2.0;
        }
        if stop.load(std::sync::atomic::Ordering::Acquire) {
            return Err("interrupted".into());
        }
        let r = send().map_err(|e| e.to_string())?;
        let status = r.status;
        let text =
            String::from_utf8(r.bytes().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let data = serde_json::from_str(&text).unwrap_or(J::String(text));
        if status == 200 || !retryable(status, &data) {
            return Ok((status, data));
        }
        last = (status, data);
    }
    Ok(last)
}

/// `_handle_error_response`'s RefreshError message.
fn error_details(data: &J) -> String {
    match data {
        J::String(s) => s.clone(),
        J::Object(m) if m.contains_key("error") => format!(
            "{}: {}",
            py_str(&m["error"]),
            m.get("error_description").map_or("None".into(), py_str)
        ),
        v => py_dumps(v),
    }
}

fn py_str(v: &J) -> String {
    match v {
        J::String(s) => s.clone(),
        J::Null => "None".into(),
        v => py_dumps(v),
    }
}

/// `jwt_grant`'s token and expiry, formatted as token_from_key.py prints them.
fn grant_output(data: &J) -> Result<(J, i64), String> {
    let m = match data {
        J::Object(m) => m,
        _ => return Err("the token response is not a JSON object".into()),
    };
    let token = m
        .get("access_token")
        .ok_or("no access token in the response")?;
    let secs = match m.get("expires_in") {
        None | Some(J::Null) => {
            return Err("no expires_in in the response".into());
        }
        Some(J::String(s)) => {
            s.trim()
                .parse::<i64>()
                .map_err(|_| format!("expires_in is not a number: {s:?}"))? as f64
        }
        Some(J::Bool(b)) => *b as i64 as f64,
        Some(J::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(v) => {
            return Err(format!("expires_in is not a number: {}", py_dumps(v)));
        }
    };
    Ok((token.clone(), expiry_after(now(), secs)))
}

#[cfg(test)]
mod tests;
