//! `Qt.openUrlExternally` opens `Qt.resolvedUrl(url)`: an address without a scheme is one of
//! the document that calls, and `xdg-open` is started with `QUrl::toEncoded()` of it, spaces
//! and everything above ASCII escaped. `expected.qt` is what Qt 6.12 starts `xdg-open` with
//! for every case of `Open.qml` (`qt.sh`), `expected.qs` the same for Quickshell 0.3.2, whose
//! url interceptor the address goes through (`qs.sh`); `DIR` in them is the directory of the
//! document, `UP` the one above. An address without a scheme used to be opened as written.
use qml::runtime::Runtime;
use qml::value::Value;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// What every case answers and starts `xdg-open` with, as the lines of `expected`: in a
/// process of its own, with an `xdg-open` first in PATH that reports through a FIFO and no
/// session bus, so no portal.
fn same(test: &str, shell: bool, expected: &str, waits: &[&str]) {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/openurl_relative");
    if !testenv::in_own_process(test) {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        let (bin, doc) = (d.join("bin"), d.join("up/doc"));
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&doc).unwrap();
        std::fs::copy(fixture.join("Open.qml"), doc.join("Open.qml")).unwrap();
        let calls = d.join("calls");
        let made = std::process::Command::new("mkfifo").arg(&calls).status();
        assert!(made.unwrap().success());
        testenv::install_script(
            &bin.join("xdg-open"),
            &format!(
                "#!/bin/sh\nprintf '%s|%s\\n' \"$#\" \"$1\" > '{}'\n",
                calls.display()
            ),
        );
        let path = std::env::var("PATH").unwrap_or_default();
        testenv::run_in_own_process(
            test,
            [
                ("PATH", format!("{}:{path}", bin.display())),
                (
                    "DBUS_SESSION_BUS_ADDRESS",
                    format!("unix:path={}", d.join("no-bus").display()),
                ),
                ("OPENURL_DIR", d.display().to_string()),
            ],
        );
        return;
    }
    let d = std::path::PathBuf::from(std::env::var_os("OPENURL_DIR").unwrap());
    let (up, doc) = (d.join("up"), d.join("up/doc"));
    // Open for writing too: each read blocks until the next `xdg-open` writes its line.
    let mut fifo = std::fs::OpenOptions::new();
    let fifo = fifo.read(true).write(true).open(d.join("calls"));
    let mut calls = BufReader::new(fifo.unwrap());

    let mut rt = Runtime::new(shell.then(|| doc.clone()));
    let root = rt.load_file(&doc.join("Open.qml")).unwrap();
    // A line is `case`, `kind`, what the call answered, what `xdg-open` was given: the
    // cases are run in the order of the lines.
    let want = std::fs::read_to_string(fixture.join(expected)).unwrap();
    let want: Vec<&str> = want.lines().collect();
    let mut ours = Vec::new();
    for (at, line) in want.iter().enumerate() {
        let said = (rt.call_method(root, "go", vec![Value::Num(at as f64)]))
            .unwrap()
            .unwrap()
            .to_js_string();
        let mut arg = String::from("-\n");
        if said == "true" {
            arg.clear();
            calls.read_line(&mut arg).unwrap();
        }
        let named: Vec<&str> = line.split('\t').take(2).collect();
        ours.push(format!(
            "{}\t{said}\t{}",
            named.join("\t"),
            arg.trim_end_matches('\n')
                .replace(doc.to_str().unwrap(), "DIR")
                .replace(up.to_str().unwrap(), "UP"),
        ));
    }
    // The cases of `waits` are known to differ: one that stopped differing fails the test,
    // for its mark to be removed.
    let waiting = |line: &str| waits.iter().any(|c| line.starts_with(&format!("{c:?}\t")));
    let mut differ = Vec::new();
    for (w, o) in want.iter().zip(&ours) {
        match (w == o, waiting(w)) {
            (true, true) => differ.push(format!("no longer differs, yet marked: {w}")),
            (false, true) => eprintln!("WAITS {expected}: {w}\n       ours: {o}"),
            (false, false) => differ.push(format!("{expected}: {w}\n       ours: {o}")),
            (true, false) => {}
        }
    }
    assert!(
        differ.is_empty() && want.len() == ours.len(),
        "{} of {} cases differ ({} ours):\n{}",
        differ.len(),
        want.len(),
        ours.len(),
        differ.join("\n")
    );
}

#[test]
fn what_xdg_open_is_started_with_is_qt_s() {
    const TEST: &str = concat!(module_path!(), "::what_xdg_open_is_started_with_is_qt_s");
    same(TEST, false, "expected.qt", &[]);
}

#[test]
fn what_xdg_open_is_started_with_is_quickshell_s() {
    const TEST: &str = concat!(
        module_path!(),
        "::what_xdg_open_is_started_with_is_quickshell_s"
    );
    same(TEST, true, "expected.qs", &[]);
}
