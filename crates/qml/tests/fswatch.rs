//! FileView `watchChanges` / FolderListModel equivalence with real Quickshell: runs the
//! scenario of tools/reference/fswatch/steps.sh against tests/fswatch/watch.qml and compares each step's
//! signal counts and listing with tests/fswatch/qs-expected.txt (recorded by
//! tools/reference/fswatch/record-qs.sh under `qs`).
mod event_loop;
use event_loop::Loop;
use qml::runtime::Runtime;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const COUNTERS: [&str; 8] = [
    "na", "nlink", "nnd", "nctrl", "nself", "ntrig", "nad", "nwr",
];

fn steps() -> Vec<(String, String)> {
    let f = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/reference/fswatch/steps.sh");
    std::fs::read_to_string(f)
        .unwrap()
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let (n, c) = l.split_once('\t').unwrap();
            (n.to_string(), c.trim_start().to_string())
        })
        .collect()
}

fn run(base: &Path, cmd: &str) {
    let ok = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(base)
        .status()
        .unwrap();
    assert!(ok.success(), "{cmd}");
}

/// Years other than the scenario's fixed `touch -d 2001` one are "now": the recording's year.
fn normalize(row: &str) -> String {
    row.split(' ')
        .map(|f| {
            f.split(',')
                .map(|e| match e.rsplit_once(':') {
                    Some((h, y)) if y.len() == 4 && y != "2001" => format!("{h}:now"),
                    _ => e.to_string(),
                })
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Deliberate deviation: Quickshell reports a FileView's own write (`self` via setText, `ad`
/// via writeAdapter) as fileChanged, which makes Config.qml's reload revert newer edits; the
/// native port does not (tests/fileview_echo.rs). Only own writes touch those two files here.
fn no_own_write_echo(row: String) -> String {
    let mut f: Vec<&str> = row.split(' ').collect();
    if !row.starts_with('#') && f.len() > 7 {
        f[5] = "0";
        f[7] = "0";
    }
    f.join(" ")
}

/// Deliberate deviation: QFileSystemWatcher cannot watch a directory that does not exist yet,
/// so Quickshell never sees `nd/x`; the native port parks the path on its nearest existing
/// ancestor and follows it once `nd` appears (crates/qml/src/fswatch.rs, the original's first run before
/// switchwall creates `generated/`).
fn follows_missing_dir(row: String) -> String {
    let mut f: Vec<&str> = row.split(' ').collect();
    if matches!(f[0], "nodir_create" | "nodir_write") {
        f[3] = "1";
    }
    f.join(" ")
}

struct Shell {
    rt: Runtime,
    root: usize,
    lp: Loop,
}

impl Shell {
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
}

#[test]
fn matches_quickshell() {
    let base: PathBuf =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("fswatch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let steps = steps();
    run(&base, &steps[0].1);
    let qml = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fswatch/watch.qml"),
    )
    .unwrap()
    .replace("@BASE@", base.to_str().unwrap());
    // A config directory: Quickshell.Io's FileView.qml resolves from the module like in the shell.
    let cfg = base.join("cfg");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(cfg.join("shell.qml"), qml).unwrap();
    let mut rt = Runtime::new(Some(cfg.clone()));
    let root = rt.load_file(&cfg.join("shell.qml")).unwrap();
    rt.settle();
    let lp = Loop::new(&mut rt);
    let mut sh = Shell { rt, root, lp };
    sh.pump(300);
    let mut got = vec!["# step a link nd ctrl self trig ad wr fl".to_string()];
    for (name, cmd) in &steps[1..] {
        let before: Vec<f64> = COUNTERS
            .iter()
            .map(|c| sh.get(c).parse().unwrap())
            .collect();
        run(&base, cmd);
        sh.pump(300);
        let mut row = name.clone();
        for (c, b) in COUNTERS.iter().zip(before) {
            let n: f64 = sh.get(c).parse().unwrap();
            row += &format!(" {}", n - b);
        }
        row += &format!(" {}", sh.get("fl"));
        got.push(row);
    }
    let want = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fswatch/qs-expected.txt"),
    )
    .unwrap();
    let want: Vec<String> = want
        .lines()
        .map(normalize)
        .map(no_own_write_echo)
        .map(follows_missing_dir)
        .collect();
    let got: Vec<String> = got.iter().map(|r| normalize(r)).collect();
    let _ = std::fs::remove_dir_all(&base);
    let diff: Vec<String> = want
        .iter()
        .zip(&got)
        .filter(|(w, g)| w != g)
        .map(|(w, g)| format!("qs:     {w}\nnative: {g}"))
        .collect();
    assert!(
        diff.is_empty() && want.len() == got.len(),
        "\n{}",
        diff.join("\n")
    );
}

/// Idle cost of the scenario's watches: `poll_native` time per 60 Hz frame with nothing
/// changing (`cargo test --features live -p qml --test fswatch idle -- --nocapture`, under
/// `strace -fc` for syscalls).
#[test]
#[cfg_attr(
    not(feature = "live"),
    ignore = "live: --features live, docs/benchmarks.md"
)]
fn idle() {
    let base: PathBuf =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("fswatch-idle-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    run(&base, &steps()[0].1);
    let qml = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fswatch/watch.qml"),
    )
    .unwrap()
    .replace("@BASE@", base.to_str().unwrap())
    .replace("running: true", "running: false");
    let cfg = base.join("cfg");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(cfg.join("shell.qml"), qml).unwrap();
    let mut rt = Runtime::new(Some(cfg.clone()));
    rt.load_file(&cfg.join("shell.qml")).unwrap();
    rt.settle();
    for _ in 0..60 {
        rt.poll_native();
    }
    let frames = 600;
    let mut spent = Duration::ZERO;
    for _ in 0..frames {
        let t = Instant::now();
        rt.poll_native();
        spent += t.elapsed();
    }
    let _ = std::fs::remove_dir_all(&base);
    println!(
        "IDLE poll_native: {:.1} us/frame over {frames} frames",
        spent.as_secs_f64() * 1e6 / frames as f64
    );
}
