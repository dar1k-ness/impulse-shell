//! swbench <sandbox home> <switchwall args..>: the Rust switchwall against a sandbox HOME with
//! no-op side effects; prints how many external programs the live session would start.
use colorgen::switchwall::{self as sw, Paths};
use colorgen::sys::Sys;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Count(PathBuf, AtomicUsize);

impl Sys for Count {
    fn hypr_json(&self, _: &str) -> Option<serde_json::Value> {
        serde_json::from_str(r#"[{"name":"DP-1","width":1920,"height":1080,"x":0,"y":0,"scale":1.0,"focused":true}]"#).ok()
    }
    fn gsettings_get(&self, _: &str) -> Option<String> {
        Some("prefer-dark".into())
    }
    fn gsettings_set(&self, _: &str, _: &str) {}
    fn run(&self, argv: &[String], _: Option<&Path>) -> Option<String> {
        self.1.fetch_add(1, Ordering::Relaxed);
        Some(if argv[0] == "hyprpicker" {
            "#3A5F0B\n".into()
        } else {
            String::new()
        })
    }
    fn spawn(&self, _: &[String]) {
        self.1.fetch_add(1, Ordering::Relaxed);
    }
    fn has_command(&self, _: &str) -> bool {
        true
    }
    fn kill_exact_wait(&self, _: &str, _: i32) {}
    fn signal_named(&self, _: &str, _: i32) {}
    fn ttys(&self) -> Vec<PathBuf> {
        vec![self.0.join("pts/0"), self.0.join("pts/1")]
    }
    fn write_tty(&self, tty: &Path, data: &[u8]) {
        let _ = std::fs::write(tty, data);
    }
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

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let home = PathBuf::from(&a[0]);
    let paths = Paths {
        config_home: home.join(".config"),
        cache_home: home.join(".cache"),
        state_home: home.join(".local/state"),
        script_dir: home.join(".config/rustshell/impulse-shell/scripts/colors"),
        home: home.clone(),
    };
    let c = Arc::new(Count(home, AtomicUsize::new(0)));
    let sys: Arc<dyn Sys> = c.clone();
    let rc = sw::main(&a[1..], &paths, &sys);
    println!("rc={rc} external={}", c.1.load(Ordering::Relaxed));
}
