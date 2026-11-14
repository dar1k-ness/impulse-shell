//! The authcheck tool runs its file like the shell does: QML timers fire at wall-clock ms, and
//! each stdin line reaches the root's `input` as it arrives. The tests wait on the tool's exit.
#![cfg(feature = "interp")]
use std::io::Write;
use std::process::{Command, Stdio};

/// Runs authcheck on `qml` with `input` on stdin; its stderr once it exits.
fn run(qml: &str, input: &[u8]) -> String {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("main.qml");
    std::fs::write(&f, qml).unwrap();
    let mut c = Command::new(env!("CARGO_BIN_EXE_authcheck"))
        .arg(&f)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(input).unwrap();
    let out = c.wait_with_output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(out.status.success(), "{}: {err}", out.status);
    err
}

#[test]
fn timers_fire() {
    let err = run(
        "import QtQuick\nItem {\n    property bool quit: false\n    \
         Timer { interval: 30; running: true; onTriggered: { console.log(\"fired\"); quit = true } }\n}\n",
        b"",
    );
    assert!(err.contains("console: fired"), "{err}");
}

#[test]
fn stdin_lines_reach_input_in_order() {
    let err = run(
        "import QtQuick\nItem {\n    property bool quit: false\n    property string input\n    \
         property int inputSeq\n    property var got: []\n    onInputSeqChanged: {\n        \
         got.push(input)\n        if (input === \"quit\") { console.log(\"got \" + got.join(\",\")); quit = true }\n    \
         }\n}\n",
        b"a\na\n\xd0\xb1\nquit\n",
    );
    assert!(err.contains("console: got a,a,б,quit"), "{err}");
}

#[test]
fn a_timer_started_by_input_fires_later() {
    let err = run(
        "import QtQuick\nItem {\n    property bool quit: false\n    property string input\n    \
         property int inputSeq\n    onInputSeqChanged: t.start()\n    \
         Timer { id: t; interval: 30; onTriggered: { console.log(\"after \" + input); quit = true } }\n}\n",
        b"go\n",
    );
    assert!(err.contains("console: after go"), "{err}");
}
