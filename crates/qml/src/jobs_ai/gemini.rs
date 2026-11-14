//! What `gemini-translate.sh` and `gemini-categorize-wallpaper.sh` share: the keyring lookup
//! (`secret-tool lookup … | jq -r '.apiKeys.gemini'`), `notify-send` (`crate::libnotify` on the
//! shared session bus) and the `curl` POST to `generateContent`.
use super::jq;

pub const ENDPOINT: &str = "https://generativelanguage.googleapis.com";

/// The script's environment (`$HOME`, `$XDG_CONFIG_HOME`, `$GEMINI_MODEL`, ...).
pub type Env<'a> = &'a dyn Fn(&str) -> Option<String>;

/// `${name:-default}`.
pub fn var_or(env: Env, name: &str, default: &str) -> String {
    env(name)
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.to_string())
}

/// `$(cmd)`: bash drops NUL bytes and trailing newlines.
pub fn subst(b: &[u8]) -> String {
    let b: Vec<u8> = b.iter().copied().filter(|&c| c != 0).collect();
    let s = String::from_utf8_lossy(&b);
    s.trim_end_matches('\n').to_string()
}

/// The secret `secret-tool lookup application rusty-impulse` prints (libsecret: DH
/// session, else plain; the first unlocked match, else the first locked one, unlocked).
fn keyring() -> Result<Option<Vec<u8>>, String> {
    crate::keyring::secret_lookup(&[crate::keyring::APP_ATTR]).map_err(|e| e.to_string())
}

/// `API_KEY=$(secret-tool lookup 'application' 'rusty-impulse' | jq -r '.apiKeys.gemini')`.
pub fn api_key(err: &mut dyn FnMut(String)) -> String {
    let secret = match keyring() {
        Ok(s) => s.unwrap_or_default(),
        Err(e) => {
            err(format!("secret-tool: {e}\n"));
            Vec::new()
        }
    };
    let (vals, perr) = jq::parse_stream(&String::from_utf8_lossy(&secret));
    let mut out = String::new();
    for (v, line) in vals {
        match jq::field(&v, "apiKeys").and_then(|v| jq::field(&v, "gemini")) {
            Ok(v) => out += &jq::raw(&v),
            Err(e) => err(format!("jq: error (at <stdin>:{line}): {e}\n")),
        }
    }
    if let Some(e) = perr {
        err(format!("{e}\n"));
    }
    subst(out.as_bytes())
}

/// `notify-send "$summary" "$body" -a "$app"`: the exit code.
pub fn notify(summary: &str, body: &str, app: &str, err: &mut dyn FnMut(String)) -> i32 {
    match crate::libnotify::notify_session(summary, body, app) {
        Some(_) => 0,
        None => {
            err("Failed to send notification\n".into());
            1
        }
    }
}

/// `response=$(curl "$base/v1beta/models/$model:generateContent" -H "x-goog-api-key: $key"
/// -H 'Content-Type: application/json' -X POST -d "$payload" 2> /dev/null)`.
pub fn generate(base: &str, model: &str, key: &str, payload: &str) -> String {
    let url = format!("{base}/v1beta/models/{model}:generateContent");
    let headers = [
        ("x-goog-api-key".to_string(), key.to_string()),
        ("Content-Type".to_string(), "application/json".to_string()),
    ];
    let body = crate::http::send(
        "POST",
        &url,
        &headers,
        Some(payload.as_bytes()),
        crate::http::Opts::default(),
    )
    .ok()
    .and_then(|r| r.bytes().ok())
    .unwrap_or_default();
    subst(&body)
}
