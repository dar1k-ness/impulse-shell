//! FileView `watchChanges` keeps following its path when the directory holding it is replaced
//! (the original's switchwall/harness: `rm -rf generated; cp -a gen.golden generated`, or a rename over
//! it) or does not exist yet: the watch moves to the new inode and the next write is seen.
mod event_loop;
use event_loop::Loop;
use qml::runtime::Runtime;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Shell {
    rt: Runtime,
    root: usize,
    lp: Loop,
    base: PathBuf,
}

impl Shell {
    /// A FileView on `base/gen/colors.json` mirroring MaterialThemeLoader: reload on change.
    fn new(tag: &str, setup: &str) -> Shell {
        let base = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("fswatch-follow-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        sh(&base, setup);
        let cfg = base.join("cfg");
        std::fs::create_dir_all(&cfg).unwrap();
        let qml = r#"
import QtQuick
import Rustshell
import Rustshell.Io
ShellRoot {
    id: root
    property int n: 0
    property string bg: ""
    FileView {
        id: fv
        path: "@BASE@/gen/colors.json"
        watchChanges: true
        onFileChanged: { root.n++; reload() }
        onLoaded: { try { root.bg = JSON.parse(fv.text()).bg } catch (e) { root.bg = "?" } }
        onLoadFailed: root.bg = "-"
    }
}
"#
        .replace("@BASE@", base.to_str().unwrap());
        std::fs::write(cfg.join("shell.qml"), qml).unwrap();
        let mut rt = Runtime::new(Some(cfg.clone()));
        let root = rt.load_file(&cfg.join("shell.qml")).unwrap();
        rt.settle();
        let lp = Loop::new(&mut rt);
        let mut s = Shell { rt, root, lp, base };
        s.pump(200);
        s
    }
    /// Runs the loop through `ms` of its clock, on the events the steps cause.
    fn pump(&mut self, ms: u64) {
        self.lp.advance(&mut self.rt, ms as f64);
    }
    fn get(&mut self, p: &str) -> String {
        self.rt
            .world
            .borrow_mut()
            .read(self.root, p)
            .map(|v| v.to_js_string())
            .unwrap_or_default()
    }
    /// Runs `cmd` in the base directory, lets the loop see it, returns (fileChanged count, bg).
    fn step(&mut self, cmd: &str) -> (u32, String) {
        let n0: u32 = self.get("n").parse().unwrap();
        sh(&self.base, cmd);
        self.pump(250);
        (self.get("n").parse::<u32>().unwrap() - n0, self.get("bg"))
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn sh(base: &Path, cmd: &str) {
    let ok = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(base)
        .status()
        .unwrap();
    assert!(ok.success(), "{cmd}");
}

const DARK: &str = "mkdir -p gen golden && echo '{\"bg\": \"dark\"}' > gen/colors.json && echo '{\"bg\": \"gold\"}' > golden/colors.json";

/// The racy harness case made deterministic: the loop runs while the directory is gone.
#[test]
fn dir_removed_then_recreated() {
    let mut s = Shell::new("rmrf", DARK);
    assert_eq!(s.get("bg"), "dark");
    let (n, _) = s.step("rm -rf gen");
    assert_eq!(n, 1, "deletion is a change");
    assert_eq!(s.step("cp -a golden gen"), (1, "gold".into()));
    assert_eq!(
        s.step("echo '{\"bg\": \"light\"}' > gen/colors.json"),
        (1, "light".into())
    );
    assert_eq!(s.step("touch gen/unrelated"), (0, "light".into()));
}

/// Replaced in one go (no poll in between), as `rm -rf generated; cp -a gen.golden generated`.
#[test]
fn dir_replaced_at_once() {
    let mut s = Shell::new("swap", DARK);
    for k in 0..5 {
        let (n, bg) = s.step("rm -rf gen && cp -a golden gen");
        assert!(n >= 1 && bg == "gold", "round {k}: {n} {bg}");
        assert_eq!(
            s.step(&format!("echo '{{\"bg\": \"l{k}\"}}' > gen/colors.json")),
            (1, format!("l{k}"))
        );
    }
}

/// A new directory renamed over the old path: the old inode (now gen.old) is no longer
/// followed, the new one is, and identical content at the new inode is not reported.
#[test]
fn dir_renamed_over() {
    let mut s = Shell::new("mv", DARK);
    assert_eq!(
        s.step("cp -a golden new && mv gen gen.old && mv new gen"),
        (1, "gold".into())
    );
    assert_eq!(
        s.step("echo '{\"bg\": \"stale\"}' > gen.old/colors.json"),
        (0, "gold".into())
    );
    assert_eq!(
        s.step("echo '{\"bg\": \"light\"}' > gen/colors.json"),
        (1, "light".into())
    );
    assert_eq!(
        s.step("cp -a gen same && mv gen gen.prev && mv same gen"),
        (0, "light".into()),
        "same text at a new inode"
    );
    assert_eq!(
        s.step("echo '{\"bg\": \"dark\"}' > gen/colors.json"),
        (1, "dark".into())
    );
}

/// Directory moved away and back later: parked on the parent meanwhile.
#[test]
fn dir_moved_away_and_back() {
    let mut s = Shell::new("away", DARK);
    let (n, _) = s.step("mv gen gen.away");
    assert_eq!(n, 0, "the file itself did not change");
    assert_eq!(
        s.step("echo '{\"bg\": \"x\"}' > gen.away/colors.json"),
        (0, "dark".into())
    );
    assert_eq!(s.step("mv gen.away gen"), (1, "x".into()));
    assert_eq!(
        s.step("echo '{\"bg\": \"y\"}' > gen/colors.json"),
        (1, "y".into())
    );
}

/// The original's first run: the generated directory does not exist until switchwall writes it.
#[test]
fn dir_created_later() {
    let mut s = Shell::new("late", "true");
    assert_eq!(s.get("bg"), "-");
    assert_eq!(s.step("mkdir -p x/y"), (0, "-".into()));
    assert_eq!(
        s.step("mkdir -p gen && echo '{\"bg\": \"new\"}' > gen/colors.json"),
        (1, "new".into())
    );
    assert_eq!(
        s.step("echo '{\"bg\": \"w\"}' > gen/colors.json"),
        (1, "w".into())
    );
}
