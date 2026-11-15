//! XDG base directories with QStandardPaths' exact rules: `QDir::homePath()` is `$HOME`
//! cleaned, or `/` when it is empty; an `XDG_*_HOME` is used verbatim when absolute,
//! otherwise `homePath() + "/<default>"` (so `HOME=` gives `//.config`, like Qt).

use std::path::PathBuf;

/// A lookup of environment variables: [`var`] reads the process environment.
pub type Env = dyn Fn(&str) -> Option<String>;

/// The process environment as an [`Env`].
pub fn var(k: &str) -> Option<String> {
    std::env::var(k).ok()
}

/// `QDir::homePath()` under `env`.
pub fn home_str_in(env: &Env) -> String {
    match env("HOME") {
        Some(h) if !h.is_empty() => clean_path(&h),
        _ => "/".into(),
    }
}

/// `QDir::cleanPath` for the absolute paths `HOME` holds.
fn clean_path(p: &str) -> String {
    let mut out: Vec<&str> = vec![];
    for c in p.split('/') {
        match c {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            c => out.push(c),
        }
    }
    let joined = out.join("/");
    if p.starts_with('/') {
        format!("/{joined}")
    } else if joined.is_empty() {
        ".".into()
    } else {
        joined
    }
}

fn pick(env: &Env, var: &str, def: &str) -> String {
    match env(var) {
        Some(v) if v.starts_with('/') => v,
        _ => format!("{}/{def}", home_str_in(env)),
    }
}

pub fn home() -> PathBuf {
    home_in(&var)
}

pub fn home_in(env: &Env) -> PathBuf {
    home_str_in(env).into()
}

pub fn config_home() -> PathBuf {
    config_home_in(&var)
}

pub fn config_home_in(env: &Env) -> PathBuf {
    pick(env, "XDG_CONFIG_HOME", ".config").into()
}

pub fn data_home() -> PathBuf {
    data_home_in(&var)
}

pub fn data_home_in(env: &Env) -> PathBuf {
    pick(env, "XDG_DATA_HOME", ".local/share").into()
}

pub fn cache_home() -> PathBuf {
    cache_home_in(&var)
}

pub fn cache_home_in(env: &Env) -> PathBuf {
    pick(env, "XDG_CACHE_HOME", ".cache").into()
}

pub fn state_home() -> PathBuf {
    state_home_in(&var)
}

pub fn state_home_in(env: &Env) -> PathBuf {
    pick(env, "XDG_STATE_HOME", ".local/state").into()
}

/// The one name of the shell's own directories under the XDG base directories: its settings
/// (`$XDG_CONFIG_HOME`), its tree (`$XDG_DATA_HOME`), its state (`$XDG_STATE_HOME`) and its
/// cache (`$XDG_CACHE_HOME`), and of its directories under `$XDG_RUNTIME_DIR` and `/tmp`.
/// tools/install.sh puts the tree there and moves an older install's directories over.
pub const APP: &str = "impulse-shell";

/// The installed tree, what `impulse-shell -c impulse-shell` runs: `$XDG_DATA_HOME/impulse-shell`.
pub fn shell_tree() -> PathBuf {
    shell_tree_in(&var)
}

pub fn shell_tree_in(env: &Env) -> PathBuf {
    data_home_in(env).join(APP)
}

/// `String` forms for the callers that build paths with `format!`.
pub fn s(p: PathBuf) -> String {
    p.to_string_lossy().into_owned()
}
