//! `code/material-code-set-color.sh`: point the Material Code theme of VS Code forks at the
//! generated seed color.

use std::path::{Path, PathBuf};

pub const FORKS: [&str; 7] = [
    "Code",
    "VSCodium",
    "Code - OSS",
    "Code - Insiders",
    "Cursor",
    "Antigravity",
    "Windsurf",
];
const KEY: &str = "\"material-code.primaryColor\"";

pub fn settings_paths(config_home: &Path) -> Vec<PathBuf> {
    FORKS
        .iter()
        .map(|f| config_home.join(f).join("User/settings.json"))
        .collect()
}

fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c')
}

/// `sed -E 's/("material-code.primaryColor"\s*:\s*")[^"]*(")/\1<color>\2/'` on one line.
fn replace_line(line: &str, color: &str) -> Option<String> {
    let mut from = 0;
    while let Some(i) = line[from..].find(KEY).map(|i| i + from) {
        let rest = &line[i + KEY.len()..];
        let after_ws = rest.trim_start_matches(is_space);
        if let Some(r) = after_ws.strip_prefix(':') {
            let r = r.trim_start_matches(is_space);
            if let Some(val) = r.strip_prefix('"')
                && let Some(end) = val.find('"')
            {
                let head = line.len() - val.len();
                return Some(format!("{}{color}{}", &line[..head], &val[end..]));
            }
        }
        from = i + 1;
    }
    None
}

/// The new settings text, as the script's `sed -i` edits leave it.
pub fn update(text: &str, color: &str) -> String {
    if text.contains(KEY) {
        return text
            .split_inclusive('\n')
            .map(|l| {
                let (body, nl) = l.strip_suffix('\n').map_or((l, ""), |b| (b, "\n"));
                replace_line(body, color).map_or_else(|| l.to_string(), |b| b + nl)
            })
            .collect();
    }
    let body = text.strip_suffix('\n').unwrap_or(text);
    let start = body.rfind('\n').map_or(0, |i| i + 1);
    let last = &text[start..];
    match last.find('}') {
        Some(j) => format!(
            "{}{},\n  {KEY}: \"{color}\"\n}}{}",
            &text[..start],
            &last[..j],
            &last[j + 1..]
        ),
        None => text.to_string(),
    }
}

/// Run the script's loop: `color` is `$(cat color.txt)`.
pub fn apply(config_home: &Path, color: &str) {
    for p in settings_paths(config_home) {
        let Ok(text) = std::fs::read_to_string(&p) else {
            continue;
        };
        let new = update(&text, color);
        if new != text {
            let _ = crate::fsutil::write_atomic(&p, new.as_bytes());
        }
    }
}
