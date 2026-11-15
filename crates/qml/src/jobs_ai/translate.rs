//! `Impulse/AiTranslation` (the `translationProc` of `welcome.qml`): Gemini translates the
//! shell's texts, the shipped `translations/en_US.json`, to a locale. That file lists every
//! `Translation.tr()` text of the sources; `tests/translation_keys.rs` keeps it so, the job
//! reads no source and changes nothing in the shell's tree.
use super::gemini::{self, Env};
use super::jq::{self, J};
use crate::jobs::{Job, Pipes};
use crate::runtime::World;
use std::path::{Component, Path, PathBuf};

const SOURCE_LOCALE: &str = "en_US";
const APP: &str = "Shell";

/// Lexical `cd "$dir" && pwd` / `os.path.abspath`.
fn normalize(p: &Path) -> PathBuf {
    let mut o = PathBuf::from("/");
    for c in p.components() {
        match c {
            Component::ParentDir => {
                o.pop();
            }
            Component::Normal(n) => o.push(n),
            _ => {}
        }
    }
    o
}

/// A locale name such as `ru_RU`, `zh-Hans` or `sr_RS@latin`. It names the translation file:
/// the script took it as given, so `../../x` wrote outside `translations/`.
fn valid_locale(l: &str) -> bool {
    let (base, modifier) = l.split_once('@').unwrap_or((l, "a"));
    let mut parts = base.split(['_', '-']);
    let lang = parts.next().unwrap_or("");
    (2..=3).contains(&lang.len())
        && lang.bytes().all(|b| b.is_ascii_alphabetic())
        && parts.all(|p| (2..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphanumeric()))
        && !modifier.is_empty()
        && modifier.len() <= 16
        && modifier.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// The whole script; `script` is its `$0`, `base` the Gemini endpoint.
pub fn run(
    script: &str,
    args: &[String],
    env: Env,
    base: &str,
    out: &mut dyn FnMut(String),
    err: &mut dyn FnMut(String),
) -> i32 {
    let locale = args.first().cloned().unwrap_or_default();
    if locale.is_empty() {
        out(format!("Usage: {script} <target_locale> [model]\n"));
        return 1;
    }
    if !valid_locale(&locale) {
        err(format!("Invalid locale: {locale}\n"));
        return 1;
    }
    let script_dir = normalize(Path::new(script).parent().unwrap_or(Path::new("/")));
    let home = env("HOME").unwrap_or_default();
    let config_home = gemini::var_or(env, "XDG_CONFIG_HOME", &format!("{home}/.config"));
    let shell_dir = format!("{config_home}/impulse-shell");
    let config = format!("{shell_dir}/config.json");
    let translations = normalize(&script_dir.join("../../translations"));
    let target_dir = format!("{shell_dir}/translations");
    let model = match args.get(1).filter(|m| !m.is_empty()) {
        Some(m) => m.clone(),
        None => gemini::var_or(env, "GEMINI_MODEL", "gemini-2.5-flash"),
    };

    let _ = std::fs::create_dir_all(&target_dir);

    let instruction = format!(
        "You are to translate the user interface of a **desktop shell**. Given a JSON object of key-value pairs, return a JSON with the same structure, with keys unchanged and values translated to {locale}. Be as **concise** as possible to save screen space, and make sure terminology is relevant (e.g. \"discharging\" refers to the battery status)."
    );
    let source = translations.join(format!("{SOURCE_LOCALE}.json"));
    let content = match std::fs::read(&source) {
        Ok(b) => gemini::subst(&b),
        Err(e) => {
            return failed(
                &format!(
                    "Could not read the shell's texts, {}: {e}",
                    source.display()
                ),
                err,
            );
        }
    };
    let prompt = jq::quote(&format!("{instruction}\n```\n{content}\n```\n"));
    let payload = jq::pretty(&J::Obj(vec![
        (
            "contents".into(),
            J::Arr(vec![J::Obj(vec![(
                "parts".into(),
                J::Arr(vec![J::Obj(vec![("text".into(), J::Str(prompt))])]),
            )])]),
        ),
        (
            "generationConfig".into(),
            J::Obj(vec![
                ("temperature".into(), J::Num("0".into())),
                ("responseMimeType".into(), J::Str("application/json".into())),
            ]),
        ),
    ]));
    let key = gemini::api_key(err);
    gemini::notify(
        "Translation started",
        "Will take 2 minutes, and you'll be notified when it's done, so feel free to do something else in the meantime.",
        APP,
        err,
    );
    let response = gemini::generate(base, &model, &key, &payload);
    let (text, jerr, code) = jq::gemini_text(&format!("{response}\n"));
    err(jerr);
    let target = format!("{target_dir}/{locale}.json");
    // The script wrote whatever came back (a quota or key error, an HTML page) as the
    // translation, switched the UI to it and said it was complete. Only a JSON object of
    // strings is a translation; anything else leaves the file and `language.ui` alone.
    let translated = code == 0
        && serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&text)
            .is_ok_and(|m| m.values().all(serde_json::Value::is_string));
    if !translated {
        return failed(&gemini_error(&response), err);
    }
    if let Err(e) = write_atomic(Path::new(&target), &text) {
        err(format!("{target}: {e}\n"));
        return failed(&format!("Could not save {target}: {e}"), err);
    }
    set_ui_locale(&config, &locale);
    gemini::notify(
        "Translation complete",
        &format!("Enjoy! In case you wanna refine it, the file is in {target}"),
        APP,
        err,
    )
}

/// Notifies that no translation was made; exit status 1.
fn failed(why: &str, err: &mut dyn FnMut(String)) -> i32 {
    err(format!("{why}\n"));
    gemini::notify("Translation failed", why, APP, err);
    1
}

/// Gemini's `.error.message`, or a generic reason for a reply without a usable translation.
fn gemini_error(response: &str) -> String {
    serde_json::from_str::<serde_json::Value>(response)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_owned))
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| "Gemini returned no valid translation".into())
}

/// Temporary file next to `path` + rename: a failed or interrupted write never leaves a
/// truncated translation, and the temporary file is removed on failure.
fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path.parent().unwrap_or(Path::new("/"));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(text.as_bytes())?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

/// `jq --arg locale "$l" '.language.ui = $locale' "$f" > "$f.tmp" && mv "$f.tmp" "$f"`, made
/// through the running shell's Config (`crate::shell_config`): written to the file directly, an
/// edit still pending in Config's delayed save wrote the old `language.ui` back over it. Without
/// a Config the file is edited in place under a lock, with no `.tmp` left behind on failure.
fn set_ui_locale(config: &str, locale: &str) {
    crate::shell_config::set(Path::new(config), &["language", "ui"], locale);
}

pub static ELEMENT: crate::impulse::Element = crate::impulse::Element {
    job: Some(job),
    ..crate::impulse::Element::new("AiTranslation")
};

/// `Impulse/AiTranslation`: has Gemini translate the shell's texts to `locale` and writes
/// the result to the user's translations folder (scripts/ai/gemini-translate.sh).
fn job(w: &mut World, p: usize) -> Option<Job> {
    let script = w
        .root_dir
        .as_ref()?
        .join("scripts/ai/gemini-translate.sh")
        .display()
        .to_string();
    let args = vec![crate::jobs::text(w, p, "locale")];
    Some(Box::new(move |pipes: &Pipes, _stop| {
        run(
            &script,
            &args,
            &|k| std::env::var(k).ok(),
            gemini::ENDPOINT,
            &mut |s| pipes.out(s),
            &mut |s| pipes.err(s),
        )
    }))
}

#[cfg(test)]
mod locale_tests {
    #[test]
    fn locale_names_a_file_inside_translations() {
        for l in [
            "ru_RU",
            "zh_CN",
            "pt-BR",
            "zh-Hans-CN",
            "sr_RS@latin",
            "fil",
            "de",
        ] {
            assert!(super::valid_locale(l), "{l}");
        }
        for l in [
            "",
            "../../x",
            "ru_RU/../../x",
            "r",
            "ru_",
            "ru_RU@",
            "ru RU",
            "ru_RU.json",
            "русский",
        ] {
            assert!(!super::valid_locale(l), "{l}");
        }
    }
}
