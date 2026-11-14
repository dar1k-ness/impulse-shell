//! The second engine audit's findings B2-1 and B2-3, which only a process of its own shows:
//! what the process keeps of its console output, and what `Math.random` gives in each run.
#![cfg(feature = "interp")]

use qml::runtime::Runtime;
use std::process::Stdio;

/// Loads a QML document whose `Component.onCompleted` runs `body` and returns its `log`.
fn run(body: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main.qml");
    let doc = format!(
        "import QtQuick\nQtObject {{\n    property string log: \"\"\n    \
         Component.onCompleted: {{ {body} }}\n}}\n"
    );
    std::fs::write(&main, doc).unwrap();
    let mut rt = Runtime::new(Some(dir.path().to_path_buf()));
    let root = rt.load_file(&main).unwrap();
    rt.settle();
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    rt.get(root, "log").to_js_string()
}

/// The resident memory of this process, in KiB (`VmRSS` of /proc/self/status).
fn resident_kib() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let line = status.lines().find(|l| l.starts_with("VmRSS:")).unwrap();
    line.split_whitespace().nth(1).unwrap().parse().unwrap()
}

/// B2-1: a console line is written and dropped, as in Qt. The engine kept every line in
/// `Interp.log`, which nothing read: 100 000 lines of 1000 characters left about 100 MiB
/// behind. The lines go to a closed stderr here.
#[test]
fn console_lines_are_not_kept() {
    let test = concat!(module_path!(), "::console_lines_are_not_kept");
    if !testenv::in_own_process(test) {
        let out = testenv::own_process_command(test)
            .stderr(Stdio::null())
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "{stdout}");
        assert!(stdout.contains("1 passed"), "{stdout}");
        return;
    }
    let lines = |n: u32| {
        format!(
            "var line = new Array(1001).join('x'); \
             for (var i = 0; i < {n}; i++) console.log(line); log = 'done'"
        )
    };
    // The first lines allocate what every later one reuses.
    assert_eq!(run(&lines(1000)), "done");
    let before = resident_kib();
    assert_eq!(run(&lines(100_000)), "done");
    let grown = resident_kib().saturating_sub(before);
    assert!(grown < 20 * 1024, "100 000 console lines left {grown} KiB");
}

/// B2-3: `Math.random` starts from the system's random source in each process, as QV4's
/// does. The seed was a constant: every run gave 0.8597941207808165, 0.39430133835633674,
/// 0.48058787404949177.
#[test]
fn math_random_differs_between_runs() {
    let test = concat!(module_path!(), "::math_random_differs_between_runs");
    if testenv::in_own_process(test) {
        let numbers = run("log = [Math.random(), Math.random(), Math.random()].join(' ')");
        println!("B2-3 numbers: {numbers}");
        return;
    }
    let numbers = || {
        let out = testenv::own_process_command(test)
            .arg("--nocapture")
            .stderr(Stdio::null())
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(out.status.success(), "{stdout}");
        // libtest prints the test's name before it on the same line.
        let line = (stdout.lines()).find_map(|l| Some(l.split_once("B2-3 numbers: ")?.1));
        let numbers: Vec<f64> = line
            .unwrap_or_else(|| panic!("no numbers in {stdout}"))
            .split(' ')
            .map(|n| n.parse().unwrap())
            .collect();
        assert_eq!(numbers.len(), 3, "{stdout}");
        assert!(
            numbers.iter().all(|n| (0.0..1.0).contains(n)),
            "{numbers:?}"
        );
        numbers
    };
    let (first, second) = (numbers(), numbers());
    assert_ne!(first, second, "two runs gave the same numbers");
}
