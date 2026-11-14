//! `qml/translations/en_US.json` lists the texts of every `Translation.tr()` call in the QML
//! tree, each as its own translation: the list `Impulse/AiTranslation` sends to Gemini. This
//! is where the sources are read for them (what `translations/tools/translation-manager.py`
//! did on every translation); the shell reads only the file.
//!
//! `UPDATE_TRANSLATION_KEYS=1 cargo test -p qml --features interp --test translation_keys`
//! rewrites the file from the sources.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn ri() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

/// Python's `str.isspace`.
fn py_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// `bytes(text, "utf-8").decode("unicode_escape")`; `None` where Python raises.
fn unicode_escape(t: &str) -> Option<String> {
    let b = t.as_bytes();
    let mut o = String::new();
    let mut i = 0;
    let hex = |s: &[u8]| {
        std::str::from_utf8(s)
            .ok()
            .and_then(|h| u32::from_str_radix(h, 16).ok())
    };
    while i < b.len() {
        let c = b[i];
        i += 1;
        if c != b'\\' {
            o.push(c as char);
            continue;
        }
        let e = *b.get(i)?;
        i += 1;
        match e {
            b'\n' => {}
            b'\\' => o.push('\\'),
            b'\'' => o.push('\''),
            b'"' => o.push('"'),
            b'a' => o.push('\u{7}'),
            b'b' => o.push('\u{8}'),
            b'f' => o.push('\u{c}'),
            b'n' => o.push('\n'),
            b'r' => o.push('\r'),
            b't' => o.push('\t'),
            b'v' => o.push('\u{b}'),
            b'0'..=b'7' => {
                let mut v = (e - b'0') as u32;
                for _ in 0..2 {
                    match b.get(i) {
                        Some(d @ b'0'..=b'7') => {
                            v = v * 8 + (d - b'0') as u32;
                            i += 1;
                        }
                        _ => break,
                    }
                }
                o.push(char::from_u32(v)?);
            }
            b'x' | b'u' | b'U' => {
                let n = match e {
                    b'x' => 2,
                    b'u' => 4,
                    _ => 8,
                };
                let h = b.get(i..i + n)?;
                if !h.iter().all(u8::is_ascii_hexdigit) {
                    return None;
                }
                i += n;
                o.push(char::from_u32(hex(h)?)?);
            }
            b'N' => return None,
            e => {
                o.push('\\');
                o.push(e as char);
            }
        }
    }
    Some(o)
}

fn unescape(text: &str) -> String {
    if text.contains("\\u") || text.contains("\\x") {
        return unicode_escape(text).unwrap_or_else(|| text.to_string());
    }
    text.replace("\\n", "\n")
        .replace("\\t", "\t")
        .replace("\\r", "\r")
        .replace("\\\"", "\"")
        .replace("\\'", "'")
        .replace("\\f", "\u{c}")
        .replace("\\b", "\u{8}")
        .replace("\\\\", "\\")
}

/// `Path.rglob(pattern)` (directory symlinks are not followed).
fn rglob(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut dirs = Vec::new();
    for e in rd.flatten() {
        let p = e.path();
        if p.file_name()
            .is_some_and(|n| n.to_string_lossy().ends_with(ext))
        {
            out.push(p.clone());
        }
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            dirs.push(p);
        }
    }
    for d in dirs {
        rglob(&d, ext, out);
    }
}

/// The text of every `Translation.tr("…")` call in the `.qml` and `.js` files under `source`
/// (`TranslationManager.extract_translatable_texts`).
fn extract(source: &Path) -> BTreeSet<String> {
    let s = r"[\s\x1c-\x1f]";
    let quoted = fancy_regex::Regex::new(&format!(
        r#"Translation\.tr{s}*\({s}*(["'])(((?!\1)[^\\]|\\.)*)(\1){s}*\)"#
    ))
    .unwrap();
    let backtick = fancy_regex::Regex::new(&format!(
        r"Translation\.tr{s}*\({s}*`([^`]*(?:\\.[^`]*)*?)`{s}*\)"
    ))
    .unwrap();
    let mut texts = BTreeSet::new();
    for ext in [".qml", ".js"] {
        let mut files = Vec::new();
        rglob(source, ext, &mut files);
        for f in files {
            let content =
                std::fs::read_to_string(&f).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
            let content = content.replace("\r\n", "\n").replace('\r', "\n");
            for (re, group) in [(&quoted, 2), (&backtick, 1)] {
                for c in re.captures_iter(&content).flatten() {
                    let text = c.get(group).map_or("", |m| m.as_str());
                    let clean = unescape(text);
                    let clean = clean.trim_matches(py_space);
                    if !clean.is_empty() {
                        texts.insert(clean.to_string());
                    }
                }
            }
        }
    }
    texts
}

/// A key no source has stays when its value ends in `/*keep*/`.
fn keep(v: Option<&serde_json::Value>) -> bool {
    matches!(v, Some(serde_json::Value::String(s)) if s.trim_matches(py_space).ends_with("/*keep*/"))
}

#[test]
fn en_us_lists_the_texts_of_the_sources() {
    let file = ri().join("translations/en_US.json");
    let texts = extract(&ri());
    assert!(texts.len() > 100, "{} texts found", texts.len());
    let mut listed: serde_json::Map<String, serde_json::Value> =
        serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    let missing: Vec<&String> = texts.iter().filter(|t| !listed.contains_key(*t)).collect();
    let extra: Vec<String> = listed
        .iter()
        .filter(|(k, v)| !texts.contains(*k) && !keep(Some(v)))
        .map(|(k, _)| k.clone())
        .collect();
    if std::env::var_os("UPDATE_TRANSLATION_KEYS").is_some() {
        for k in &extra {
            listed.shift_remove(k);
        }
        for k in missing {
            listed.insert(k.clone(), serde_json::Value::String(k.clone()));
        }
        std::fs::write(&file, serde_json::to_string_pretty(&listed).unwrap()).unwrap();
        return;
    }
    assert!(
        missing.is_empty() && extra.is_empty(),
        "{} is behind the sources (UPDATE_TRANSLATION_KEYS=1 rewrites it)\nnot listed: {missing:#?}\nin no source: {extra:#?}",
        file.display()
    );
}
