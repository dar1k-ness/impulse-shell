//! The QML type `HyprlandConfigFile` (module `Impulse`): the `hl.config({...})` line editor
//! behind `HyprlandConfig.set/setMany/reset/resetMany` (shellOverrides/main.lua), what
//! `scripts/hyprland/hyprconfigurator.py` did. Same matching,
//! formatting and atomic replace as the script, without bash and python processes. Edits run
//! in order on their own writer thread.

use regex::Regex;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::Sender;

/// One configurator run: `--file path (--set KEY VALUE)* (--reset KEY)*`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Edit {
    pub file: String,
    pub set: Vec<(String, String)>,
    pub reset: Vec<String>,
}

/// Python `str.isspace` for the characters `float()`, `strip()` and `re` `\s` treat as space.
fn py_space(c: char) -> bool {
    c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)
}

/// Whether Python `float(s)` accepts `s` (ASCII digits; underscores between digits).
fn py_float(s: &str) -> bool {
    let t = s.trim_matches(py_space);
    let t = t.strip_prefix(['+', '-']).unwrap_or(t);
    let l = t.to_ascii_lowercase();
    if matches!(l.as_str(), "inf" | "infinity" | "nan") {
        return true;
    }
    let b = t.as_bytes();
    let mut i = 0;
    let digits = |i: &mut usize| -> Option<usize> {
        let start = *i;
        while *i < b.len() && b[*i].is_ascii_digit() {
            *i += 1;
            if *i + 1 < b.len() && b[*i] == b'_' && b[*i + 1].is_ascii_digit() {
                *i += 1;
            }
        }
        (*i > start).then_some(*i - start)
    };
    let int = digits(&mut i).is_some();
    let mut frac = false;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        frac = digits(&mut i).is_some();
    }
    if !int && !frac {
        return false;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        if digits(&mut i).is_none() {
            return false;
        }
    }
    i == b.len()
}

pub fn format_value(v: &str) -> String {
    if v == "true" || v == "false" || py_float(v) {
        v.to_string()
    } else {
        lua_str(v)
    }
}

/// `s` as a double-quoted Lua string literal. The script pasted values in raw, so a `"`, `\\`
/// or newline from a setting broke shellOverrides/main.lua (Hyprland then skipped it) or
/// injected Lua.
pub fn lua_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_ascii_control() => out.push_str(&format!("\\{:03}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn nested(parts: &[&str], value: &str) -> String {
    match parts {
        [k] => format!("{k}={}", format_value(value)),
        [k, rest @ ..] => format!("{k}={{{}}}", nested(rest, value)),
        [] => unreachable!(),
    }
}

pub fn config_line(key: &str, value: &str) -> String {
    let parts: Vec<&str> = key.split(':').collect();
    format!("hl.config({{{}}})\n", nested(&parts, value))
}

/// The script's `^\s*hl\.config\(\{\s*a\s*=\{\s*b\s*=` prefix pattern of `key`.
fn pattern(key: &str) -> Regex {
    const S: &str = r"[\s\x1c-\x1f]*";
    let parts: Vec<String> = key
        .split(':')
        .map(|p| format!("{S}{}{S}=", regex::escape(p)))
        .collect();
    Regex::new(&format!(r"^{S}hl\.config\(\{{{}", parts.join(r"\{"))).expect("escaped pattern")
}

/// Python text-mode `readlines()`: universal newlines, line ends kept as `\n`.
fn read_lines(text: &str) -> Vec<String> {
    let t = text.replace("\r\n", "\n").replace('\r', "\n");
    t.split_inclusive('\n').map(String::from).collect()
}

/// `edit_hyprland_config`: the new file text.
pub fn apply(text: &str, set: &[(String, String)], reset: &[String]) -> String {
    // `{k: v for k, v in set_args}`: first position, last value.
    let mut sets: Vec<(String, String)> = Vec::new();
    for (k, v) in set {
        match sets.iter_mut().find(|e| e.0 == *k) {
            Some(e) => e.1 = v.clone(),
            None => sets.push((k.clone(), v.clone())),
        }
    }
    let resets: Vec<Regex> = reset.iter().map(|k| pattern(k)).collect();
    let set_pats: Vec<Regex> = sets.iter().map(|(k, _)| pattern(k)).collect();
    let mut found = vec![false; sets.len()];
    let mut out: Vec<String> = Vec::new();
    for line in read_lines(text) {
        if resets.iter().any(|p| p.is_match(&line)) {
            continue;
        }
        if let Some(i) = set_pats.iter().position(|p| p.is_match(&line)) {
            out.push(config_line(&sets[i].0, &sets[i].1));
            found[i] = true;
            continue;
        }
        out.push(line);
    }
    for (i, (k, v)) in sets.iter().enumerate() {
        if !found[i] {
            if let Some(last) = out.last_mut()
                && !last.ends_with('\n')
            {
                last.push('\n');
            }
            out.push(config_line(k, v));
        }
    }
    out.concat()
}

fn expand_user(p: &str) -> PathBuf {
    match (p.strip_prefix('~'), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with('/') => {
            PathBuf::from(format!("{}{rest}", home.to_string_lossy()))
        }
        _ => PathBuf::from(p),
    }
}

/// The script's `__main__`: `[[EMPTY]]` values reset, then edit and atomically replace.
pub fn run(e: &Edit) -> Result<(), String> {
    let mut set = Vec::new();
    let mut reset = e.reset.clone();
    for (k, v) in &e.set {
        if v == "[[EMPTY]]" {
            reset.push(k.clone());
        } else {
            set.push((k.clone(), v.clone()));
        }
    }
    if set.is_empty() && reset.is_empty() {
        return Err("Must specify at least one key to set or reset.".into());
    }
    let path = expand_user(&e.file);
    let text = if path.exists() {
        std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?
    } else {
        String::new()
    };
    let new = apply(&text, &set, &reset);
    write_atomic(&path, &new).map_err(|e| format!("Error saving file: {e}"))
}

/// `NamedTemporaryFile(dir=...)` + chmod (the old file's mode, else 0644) + `os.replace`;
/// the temporary file is removed on failure.
fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let abs = std::path::absolute(path)?;
    let dir = abs.parent().unwrap_or(Path::new("/"));
    std::fs::create_dir_all(dir)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(text.as_bytes())?;
    tmp.flush()?;
    let mode = match std::fs::metadata(path) {
        Ok(m) => m.permissions().mode() & 0o7777,
        Err(_) => 0o644,
    };
    std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(mode))?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

enum Msg {
    Edit(Edit),
    Flush(Sender<()>),
}

static WRITER: Mutex<Option<Sender<Msg>>> = Mutex::new(None);

impl Msg {
    fn handle(self) {
        match self {
            Msg::Edit(e) => {
                if let Err(m) = run(&e) {
                    elog::eprintln!("hyprconfigurator: {m}");
                }
            }
            Msg::Flush(done) => {
                let _ = done.send(());
            }
        }
    }
}

fn send(m: Msg) {
    let mut w = WRITER.lock().unwrap();
    let m = match w.as_ref() {
        Some(tx) => match tx.send(m) {
            Ok(()) => return,
            Err(err) => err.0,
        },
        None => m,
    };
    let (tx, rx) = std::sync::mpsc::channel::<Msg>();
    let spawned = std::thread::Builder::new()
        .name("hyprconf".into())
        .spawn(move || rx.into_iter().for_each(Msg::handle));
    match spawned {
        Ok(_) => {
            let _ = tx.send(m);
            *w = Some(tx);
        }
        // The system refused the thread (`EAGAIN`): no writer runs, so nothing is queued
        // before `m`. It is handled here, under the lock, in the order asked; the next one
        // starts the thread again.
        Err(e) => {
            elog::eprintln!("hyprconfigurator: cannot start the hyprconf thread: {e}");
            m.handle();
        }
    }
}

/// Queue `e` on the writer thread (`Rustshell.execDetached` of the script: fire and forget).
/// Without a thread from the system the edit is made before this returns.
pub fn submit(e: Edit) {
    send(Msg::Edit(e));
}

/// Waits until the edits queued so far are written.
pub fn flush() {
    // Nothing was ever queued: no writer thread to start just to answer.
    if WRITER.lock().unwrap().is_none() {
        return;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    send(Msg::Flush(tx));
    // The writer answers every Flush in order; its Sender is dropped if the thread dies.
    let _ = rx.recv();
}

// ---- the QML type `HyprlandConfigFile` ----

use crate::runtime::World;
use crate::value::Value;

/// The own enumerable properties of `v` (array elements first), as text.
fn entries(v: &Value) -> Vec<(String, String)> {
    let Value::Obj(o) = v else {
        return Vec::new();
    };
    let o = o.borrow();
    let elements = o.array.iter().flatten().enumerate();
    elements
        .map(|(i, v)| (i.to_string(), v.to_js_string()))
        .chain(o.keys.iter().map(|k| {
            let v = o.props.get(k).map(|v| v.to_js_string());
            (k.to_string(), v.unwrap_or_default())
        }))
        .collect()
}

/// `HyprlandConfigFile` of the `Impulse` module: `set(entries)` writes each key of `entries`
/// as its `hl.config({...})` line of the file `path`, `reset(keys)` removes the lines of
/// `keys`. Fire and forget, in the order asked.
pub static FILE: crate::impulse::Element = crate::impulse::Element {
    methods: &[
        ("reset", |w, item, args| {
            let mut e = edit_of(w, item);
            e.reset = given(args).into_iter().map(|(_, k)| k).collect();
            if !hooked("reset", &e.file, e.reset.clone()) {
                submit(e);
            }
            Value::Undefined
        }),
        ("set", |w, item, args| {
            let mut e = edit_of(w, item);
            e.set = given(args);
            let shown = e.set.iter().map(|(k, v)| format!("{k}={v}")).collect();
            if !hooked("set", &e.file, shown) {
                submit(e);
            }
            Value::Undefined
        }),
    ],
    ..crate::impulse::Element::new("HyprlandConfigFile")
};

/// [`crate::impulse::hooked`] for `method` of the element whose file is `file`: under a
/// spawn hook the file is not written.
fn hooked(method: &str, file: &str, rest: Vec<String>) -> bool {
    let args: Vec<&str> = std::iter::once(file)
        .chain(rest.iter().map(String::as_str))
        .collect();
    crate::impulse::hooked("HyprlandConfigFile", method, &args)
}

fn edit_of(w: &mut World, item: usize) -> Edit {
    Edit {
        file: w
            .read(item, "path")
            .map(|v| v.to_js_string())
            .unwrap_or_default(),
        ..Edit::default()
    }
}

fn given(args: &[Value]) -> Vec<(String, String)> {
    entries(args.first().unwrap_or(&Value::Undefined))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The system refuses the `hyprconf` thread (the limit on threads is reached): the edit is
    /// made in place instead of being dropped, and the refusal is in the log. In a process of
    /// its own: the writer is started once per process, and the test reads the process's
    /// stderr.
    #[test]
    fn a_refused_thread_makes_the_edit_in_place() {
        const TEST: &str = concat!(module_path!(), "::a_refused_thread_makes_the_edit_in_place");
        if !crate::testenv::in_own_process(TEST) {
            crate::testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("overrides.lua");
        crate::testenv::refuse_new_threads();
        let ((), log) = crate::testenv::stderr_with(|| {
            submit(Edit {
                file: file.to_string_lossy().into_owned(),
                set: vec![("general:gaps_in".into(), "7".into())],
                reset: vec![],
            });
        });
        flush();
        assert_eq!(
            std::fs::read_to_string(&file).ok(),
            Some(apply("", &[("general:gaps_in".into(), "7".into())], &[])),
            "the edit was dropped"
        );
        assert!(
            log.contains("hyprconfigurator: cannot start the hyprconf thread:"),
            "stderr: {log:?}"
        );
    }
}
