//! `applycolor.sh`: fill the terminal templates from `material_colors.scss`, reload kitty and
//! recolor open terminals.

use crate::fsutil::write_atomic;
use crate::switchwall::Io;
use crate::sys::Sys;
use std::path::Path;

pub const TERM_ALPHA: &str = "100";

/// `cut -d: -f1` / `cut -d: -f2 | cut -d' ' -f2 | cut -d';' -f1` of each line.
pub fn scss_pairs(scss: &str) -> Vec<(String, String)> {
    let field = |s: &str, d: char, n: usize| -> String {
        if !s.contains(d) {
            return s.to_string();
        }
        s.split(d).nth(n).unwrap_or("").to_string()
    };
    scss.lines()
        .filter(|l| !l.is_empty())
        .map(|l| {
            let name = field(l, ':', 0);
            let v = field(&field(&field(l, ':', 1), ' ', 1), ';', 0);
            (name, v)
        })
        .collect()
}

pub fn valid_scss(scss: &str) -> bool {
    scss.lines().any(|l| l.starts_with("$primary: #"))
}

/// The `sed "s/<name> #/<value without #>/g"` passes, in scss order.
pub fn fill(template: &str, pairs: &[(String, String)]) -> String {
    let mut out = template.to_string();
    for (name, value) in pairs {
        let pat = format!("{name} #");
        if out.contains(&pat) {
            out = out.replace(&pat, value.strip_prefix('#').unwrap_or(value));
        }
    }
    out
}

pub struct Paths<'a> {
    pub script_dir: &'a Path,
    pub state_dir: &'a Path,
    pub qs_config_dir: &'a Path,
    pub shell_config: &'a Path,
}

fn render(
    p: &Paths,
    io: Io,
    tpl: &str,
    out_name: &str,
    pairs: &[(String, String)],
    alpha: bool,
) -> Option<Vec<u8>> {
    let src = p.script_dir.join("terminal").join(tpl);
    let Ok(t) = std::fs::read_to_string(&src) else {
        (io.err)(format!(
            "Template file not found for {}. Skipping that.\n",
            if alpha { "Terminal" } else { "Kitty theme" }
        ));
        return None;
    };
    let mut s = fill(&t, pairs);
    if alpha {
        s = s.replace("$alpha", TERM_ALPHA);
    }
    if s.contains("#$") {
        (io.err)(format!(
            "[applycolor.sh] Unfilled colors in {}, keeping the current one{}\n",
            if alpha {
                "terminal sequences"
            } else {
                "kitty theme"
            },
            if alpha { "s" } else { "" }
        ));
        return None;
    }
    let out = p.state_dir.join("user/generated/terminal").join(out_name);
    write_atomic(&out, s.as_bytes()).ok()?;
    Some(s.into_bytes())
}

pub fn apply_kitty(p: &Paths, pairs: &[(String, String)], sys: &dyn Sys, io: Io) {
    if render(p, io, "kitty-theme.conf", "kitty-theme.conf", pairs, false).is_some() {
        sys.signal_named("kitty", libc::SIGUSR1);
    }
}

pub fn apply_anyterm(p: &Paths, pairs: &[(String, String)], sys: &dyn Sys, io: Io) {
    if let Some(seq) = render(p, io, "sequences.txt", "sequences.txt", pairs, true) {
        for t in sys.ttys() {
            sys.write_tty(&t, &seq);
        }
    }
}

fn terminal_enabled(shell_config: &Path, io: Io) -> bool {
    match std::fs::read_to_string(shell_config) {
        Err(_) => {
            (io.out)(format!(
                "Config file not found at {}. Applying terminal theming by default.\n",
                shell_config.display()
            ));
            true
        }
        Ok(t) => {
            crate::switchwall::jq_r(&t, &["appearance", "wallpaperTheming", "enableTerminal"])
                .as_deref()
                == Some("true")
        }
    }
}

/// The whole script. Returns `false` where it exits 1. `apply_kitty` runs on a thread beside
/// `apply_anyterm`, as the script backgrounds both.
pub fn applycolor(p: &Paths, sys: &dyn Sys, io: Io) -> bool {
    if !p.qs_config_dir.is_dir() {
        return false;
    }
    let scss = std::fs::read_to_string(p.state_dir.join("user/generated/material_colors.scss"))
        .unwrap_or_default();
    if !valid_scss(&scss) {
        (io.err)(
            "[applycolor.sh] material_colors.scss is empty or invalid, keeping current terminal colors\n"
                .into(),
        );
        return false;
    }
    if terminal_enabled(p.shell_config, io) {
        let pairs = scss_pairs(&scss);
        let kitty = || apply_kitty(p, &pairs, sys, io);
        std::thread::scope(|s| {
            // Refused by the system (`EAGAIN`): kitty's part runs after the terminals'.
            let beside = std::thread::Builder::new().spawn_scoped(s, kitty);
            apply_anyterm(p, &pairs, sys, io);
            if beside.is_err() {
                kitty();
            }
        });
    }
    true
}
