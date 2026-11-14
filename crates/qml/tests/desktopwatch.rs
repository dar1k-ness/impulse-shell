//! DesktopEntries directory-watch equivalence with real Quickshell: runs the scenario of
//! tools/reference/desktopwatch/steps.sh against tests/desktopwatch/apps.qml and compares
//! `applications` after each step with tests/desktopwatch/qs-expected.txt (recorded by
//! tools/reference/desktopwatch/record-qs.sh under `qs`).
mod event_loop;
use event_loop::Loop;
use qml::runtime::Runtime;
use std::path::Path;
use std::process::Command;

fn steps() -> Vec<(String, String)> {
    let f =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/reference/desktopwatch/steps.sh");
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

#[test]
fn matches_quickshell() {
    const TEST: &str = concat!(module_path!(), "::matches_quickshell");
    let steps = steps();
    if !testenv::in_own_process(TEST) {
        let dir = tempfile::Builder::new()
            .prefix("desktopwatch-")
            .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
            .unwrap();
        let base = dir.path();
        run(base, &steps[0].1);
        testenv::run_in_own_process(
            TEST,
            [
                ("XDG_DATA_HOME", base.join("home").into_os_string()),
                (
                    "XDG_DATA_DIRS",
                    format!("{0}/sys:{0}/late", base.display()).into(),
                ),
                ("LANG", "C".into()),
                ("LC_ALL", "C".into()),
            ],
        );
        return;
    }
    let base = Path::new(&std::env::var_os("XDG_DATA_HOME").unwrap())
        .parent()
        .unwrap()
        .to_path_buf();
    let cfg = base.join("cfg");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/desktopwatch/apps.qml"),
        cfg.join("shell.qml"),
    )
    .unwrap();
    let mut rt = Runtime::new(Some(cfg.clone()));
    let root = rt.load_file(&cfg.join("shell.qml")).unwrap();
    rt.settle();
    let mut lp = Loop::new(&mut rt);
    let apps = |rt: &mut Runtime| {
        rt.world
            .borrow_mut()
            .read(root, "apps")
            .map(|v| v.to_js_string())
            .unwrap_or_default()
    };
    lp.advance(&mut rt, 300.0);
    let mut got = vec![
        "# step apps".to_string(),
        format!("start {}", apps(&mut rt)),
    ];
    for (name, cmd) in &steps[1..] {
        run(&base, cmd);
        lp.advance(&mut rt, 600.0);
        got.push(format!("{name} {}", apps(&mut rt)));
    }
    let want = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/desktopwatch/qs-expected.txt"),
    )
    .unwrap();
    let want: Vec<&str> = want.lines().collect();
    let diff: Vec<String> = want
        .iter()
        .zip(&got)
        .filter(|(w, g)| *w != *g)
        .map(|(w, g)| format!("qs:     {w}\nnative: {g}"))
        .collect();
    assert!(
        diff.is_empty() && want.len() == got.len(),
        "\n{}",
        diff.join("\n")
    );
}
