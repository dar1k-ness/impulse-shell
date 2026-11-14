//! Rust jobs behind the `Impulse` types for random wallpapers, the Google service-account
//! token and translation (see `crate::jobs`).
pub(crate) mod gcloud;
pub(crate) mod gcloud_api;
mod langs;
pub(crate) mod trans;
pub(crate) mod wall;
pub use wall::{SWITCHWALL_ARG, switchwall_serve};
pub(crate) use wall::{switchwall_picker, switchwall_proc};

/// An environment variable as a shell sees it (unset and invalid UTF-8 read as empty).
fn env(k: &str) -> String {
    std::env::var(k).unwrap_or_default()
}

/// Python's `json.dumps(v)` (default separators, `ensure_ascii=True`).
fn py_dumps(v: &serde_json::Value) -> String {
    use serde_json::Value as J;
    fn s(o: &mut String, t: &str) {
        o.push('"');
        for c in t.chars() {
            match c {
                '"' => o.push_str("\\\""),
                '\\' => o.push_str("\\\\"),
                '\n' => o.push_str("\\n"),
                '\r' => o.push_str("\\r"),
                '\t' => o.push_str("\\t"),
                '\u{8}' => o.push_str("\\b"),
                '\u{c}' => o.push_str("\\f"),
                c if (c as u32) < 0x20 || (c as u32) > 0x7e => {
                    let mut b = [0u16; 2];
                    for u in c.encode_utf16(&mut b) {
                        o.push_str(&format!("\\u{u:04x}"));
                    }
                }
                c => o.push(c),
            }
        }
        o.push('"');
    }
    fn go(o: &mut String, v: &J) {
        match v {
            J::Null => o.push_str("null"),
            J::Bool(b) => o.push_str(if *b { "true" } else { "false" }),
            J::Number(n) => o.push_str(&n.to_string()),
            J::String(t) => s(o, t),
            J::Array(a) => {
                o.push('[');
                for (i, x) in a.iter().enumerate() {
                    if i > 0 {
                        o.push_str(", ");
                    }
                    go(o, x);
                }
                o.push(']');
            }
            J::Object(m) => {
                o.push('{');
                for (i, (k, x)) in m.iter().enumerate() {
                    if i > 0 {
                        o.push_str(", ");
                    }
                    s(o, k);
                    o.push_str(": ");
                    go(o, x);
                }
                o.push('}');
            }
        }
    }
    let mut o = String::new();
    go(&mut o, v);
    o
}

#[cfg(test)]
pub(crate) mod testutil;
