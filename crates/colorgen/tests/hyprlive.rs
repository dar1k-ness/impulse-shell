//! Hyprland Lua outputs are applied through `hyprctl eval` with autoreload paused around the
//! write, so a theme switch never makes Hyprland re-run its whole config.

use colorgen::hyprlive::{eval_request, is_hypr_lua, write};
use colorgen::sys::Sys;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

struct Hypr {
    file: PathBuf,
    /// `misc:disable_autoreload` as Hyprland reports it; `None`: no Hyprland.
    disabled: Option<bool>,
    eval_reply: &'static str,
    /// Each request with the file content at the time it arrived.
    log: Mutex<Vec<(String, String)>>,
}

impl Hypr {
    fn new(dir: &Path, disabled: Option<bool>, eval_reply: &'static str) -> Self {
        Hypr {
            file: dir.join("hypr/hyprland/colors.lua"),
            disabled,
            eval_reply,
            log: Mutex::new(Vec::new()),
        }
    }
    fn requests(&self) -> Vec<(String, String)> {
        self.log.lock().unwrap().clone()
    }
}

impl Sys for Hypr {
    fn hypr_json(&self, _cmd: &str) -> Option<serde_json::Value> {
        unreachable!()
    }
    fn hypr_request(&self, cmd: &str) -> Option<String> {
        let disabled = self.disabled?;
        let content = std::fs::read_to_string(&self.file).unwrap_or_default();
        self.log.lock().unwrap().push((cmd.to_string(), content));
        Some(match cmd {
            "j/getoption misc:disable_autoreload" => format!(
                r#"{{"option": "misc:disable_autoreload", "bool": {disabled}, "set": false}}"#
            ),
            c if c.starts_with("eval hl.config({ misc") => "ok".into(),
            c if c.starts_with("eval ") => self.eval_reply.into(),
            _ => "ok".into(),
        })
    }
    fn gsettings_get(&self, _: &str) -> Option<String> {
        None
    }
    fn gsettings_set(&self, _: &str, _: &str) {}
    fn run(&self, _: &[String], _: Option<&Path>) -> Option<String> {
        None
    }
    fn spawn(&self, _: &[String]) {}
    fn has_command(&self, _: &str) -> bool {
        false
    }
    fn kill_exact_wait(&self, _: &str, _: i32) {}
    fn signal_named(&self, _: &str, _: i32) {}
    fn ttys(&self) -> Vec<PathBuf> {
        Vec::new()
    }
    fn write_tty(&self, _: &Path, _: &[u8]) {}
    fn notify(
        &self,
        _: &str,
        _: Option<&str>,
        _: &str,
        _: &str,
        _: &[(&str, &str)],
        then: colorgen::sys::OnAction,
    ) {
        then(None);
    }
}

const NEW: &str = "hl.config({ general = { col = { active_border = \"rgba(11223377)\" } } })\n";
const PAUSE: &str = "eval hl.config({ misc = { disable_autoreload = true } })";
const RESUME: &str = "eval hl.config({ misc = { disable_autoreload = false } })";

fn tmp() -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "hyprlive-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("hypr/hyprland")).unwrap();
    std::fs::write(d.join("hypr/hyprland/colors.lua"), "old\n").unwrap();
    d
}

#[test]
fn only_lua_files_under_the_hypr_config_dir() {
    let home = Path::new("/h/.config");
    assert!(is_hypr_lua(home, &home.join("hypr/hyprland/colors.lua")));
    assert!(!is_hypr_lua(home, &home.join("hypr/hyprlock/colors.conf")));
    assert!(!is_hypr_lua(home, &home.join("fuzzel/fuzzel_theme.ini")));
    assert!(!is_hypr_lua(home, Path::new("/elsewhere/hypr/colors.lua")));
}

#[test]
fn applied_by_eval_with_autoreload_paused_around_the_write() {
    let d = tmp();
    let h = Hypr::new(&d, Some(false), "ok");
    write(&h, &h.file, NEW).unwrap();
    let r = h.requests();
    let cmds: Vec<&str> = r.iter().map(|(c, _)| c.as_str()).collect();
    assert_eq!(cmds.len(), 4, "{cmds:#?}");
    assert_eq!(cmds[0], "j/getoption misc:disable_autoreload");
    assert_eq!(
        (cmds[1], r[1].1.as_str()),
        (PAUSE, "old\n"),
        "paused before the write"
    );
    assert_eq!(cmds[2], eval_request(&h.file, NEW));
    assert_eq!(r[2].1, NEW, "evaluated after the write");
    assert_eq!(cmds[3], RESUME);
    assert!(!cmds.contains(&"reload"));
    assert_eq!(std::fs::read_to_string(&h.file).unwrap(), NEW);
}

#[test]
fn failed_eval_falls_back_to_a_reload() {
    let d = tmp();
    let h = Hypr::new(&d, Some(false), "error: boom");
    write(&h, &h.file, NEW).unwrap();
    let cmds: Vec<String> = h.requests().into_iter().map(|(c, _)| c).collect();
    assert_eq!(cmds[3], RESUME);
    assert_eq!(cmds[4], "reload");
    assert_eq!(cmds.len(), 5);
}

#[test]
fn autoreload_disabled_by_the_user_stays_disabled() {
    let d = tmp();
    let h = Hypr::new(&d, Some(true), "ok");
    write(&h, &h.file, NEW).unwrap();
    let cmds: Vec<String> = h.requests().into_iter().map(|(c, _)| c).collect();
    assert_eq!(
        cmds,
        [
            "j/getoption misc:disable_autoreload".to_string(),
            eval_request(&h.file, NEW)
        ]
    );
}

#[test]
fn without_hyprland_a_plain_write() {
    let d = tmp();
    let h = Hypr::new(&d, None, "ok");
    write(&h, &h.file, NEW).unwrap();
    assert!(h.requests().is_empty());
    assert_eq!(std::fs::read_to_string(&h.file).unwrap(), NEW);
}

#[test]
fn eval_names_unnamed_window_rules() {
    let req = eval_request(Path::new("/x/hypr/hyprland/colors.lua"), NEW);
    assert!(req.starts_with("eval local window_rule"));
    assert!(req.contains("rule.name = \"rusty-impulse colors.lua \" .. n"));
    assert!(req.contains(NEW));
    // No flag prefix: Hyprland only reads flags before a `/` that comes before any space.
    assert!(req.find(' ') < req.find('/').or(Some(usize::MAX)));
}
