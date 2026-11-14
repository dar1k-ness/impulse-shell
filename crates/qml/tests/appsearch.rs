//! Launcher app search over DesktopEntries rescans, equivalence with real Quickshell: the tree
//! LauncherSearch driven by tests/appsearch/shell.qml answers tools/reference/appsearch/queries.txt, then
//! `foo`/`fire`/`kit` after each step of tools/reference/appsearch/steps.sh (duplicate ids, parent-directory
//! rescans, action changes, mask/NoDisplay, install/uninstall); compared with
//! tests/appsearch/qs-expected.txt (recorded by tools/reference/appsearch/record-qs.sh under `qs`).
mod event_loop;
use event_loop::Loop;
use qml::runtime::Runtime;
use qml::value::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

fn manifest() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn steps() -> Vec<(String, String)> {
    std::fs::read_to_string(manifest().join("../../tools/reference/appsearch/steps.sh"))
        .unwrap()
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let (n, c) = l.split_once('\t').unwrap();
            (n.to_string(), c.to_string())
        })
        .collect()
}

fn run(base: &Path, cmd: &str) {
    let ok = Command::new("bash")
        .arg("-c")
        .arg(cmd)
        .current_dir(base)
        .status()
        .unwrap();
    assert!(ok.success(), "{cmd}");
}

#[test]
fn matches_quickshell() {
    let ri: PathBuf = manifest().join("../../qml").canonicalize().unwrap();
    const TEST: &str = concat!(module_path!(), "::matches_quickshell");
    let steps = steps();
    if !testenv::in_own_process(TEST) {
        let dir = tempfile::Builder::new()
            .prefix("appsearch-")
            .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
            .unwrap();
        let base = dir.path();
        run(base, &steps[0].1);
        testenv::run_in_own_process(
            TEST,
            [
                ("HOME", base.join("home").into_os_string()),
                ("XDG_DATA_HOME", base.join("home").into_os_string()),
                ("XDG_DATA_DIRS", base.join("sys").into_os_string()),
                ("XDG_CONFIG_HOME", base.join("config").into_os_string()),
                ("XDG_STATE_HOME", base.join("state").into_os_string()),
                ("XDG_CACHE_HOME", base.join("cache").into_os_string()),
                ("LANG", "C.UTF-8".into()),
                ("LC_ALL", "C.UTF-8".into()),
            ],
        );
        return;
    }
    let base = Path::new(&std::env::var_os("XDG_DATA_HOME").unwrap())
        .parent()
        .unwrap()
        .to_path_buf();
    qml::native::set_spawn_hook(Some(Box::new(|_| Some(String::new()))));
    // Midday for the engine: the original's night light schedule (Hyprsunset, loaded with the services)
    // decides nothing here from the hour the test runs at.
    qml::builtins::set_clock(Some(testenv::local_time_ms(12, 0)));
    let mut rt = Runtime::new(Some(ri.clone()));
    rt.load_singletons(&[
        ri.join("modules/common"),
        ri.join("modules/common/functions"),
        ri.join("services"),
    ]);
    rt.settle();
    let driver = std::fs::read_to_string(manifest().join("tests/appsearch/shell.qml")).unwrap();
    let root = rt.load_str(&driver).unwrap();
    let mut lp = Loop::new(&mut rt);
    let q = |rt: &mut Runtime, s: &str| {
        let v = rt
            .call_method(root, "q", vec![Value::Str(s.into())])
            .unwrap();
        rt.settle();
        v.map(|v| v.to_js_string())
            .unwrap_or_else(|e| format!("error: {e}"))
    };
    lp.advance(&mut rt, 1500.0);
    let mut got = vec!["# query results".to_string()];
    let queries =
        std::fs::read_to_string(manifest().join("../../tools/reference/appsearch/queries.txt"))
            .unwrap();
    for s in queries.lines() {
        let r = q(&mut rt, s);
        got.push(format!("{}\t{r}", s.chars().take(40).collect::<String>()));
    }
    got.push("# step results (foo | fire | kit)".to_string());
    let mut row = |rt: &mut Runtime, name: &str| {
        let r = format!(
            "{name} {} | {} | {}",
            q(rt, "foo"),
            q(rt, "fire"),
            q(rt, "kit")
        );
        got.push(r);
    };
    row(&mut rt, "start");
    for (name, cmd) in &steps[1..] {
        run(&base, cmd);
        lp.advance(&mut rt, 800.0);
        row(&mut rt, name);
    }
    qml::native::set_spawn_hook(None);
    let want = std::fs::read_to_string(manifest().join("tests/appsearch/qs-expected.txt")).unwrap();
    let want: Vec<&str> = want.lines().collect();
    let diff: Vec<String> = want
        .iter()
        .zip(&got)
        .filter(|(w, g)| *w != *g)
        .map(|(w, g)| format!("qs:     {w}\nnative: {g}"))
        .collect();
    assert!(
        diff.is_empty() && want.len() == got.len(),
        "\n{}\n(qs {} lines, native {})",
        diff.join("\n"),
        want.len(),
        got.len()
    );
}
