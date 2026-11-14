//! LazyLoader against the real Quickshell: the documents of `lazy_loader/` log every state
//! change, signal and completion of their LazyLoaders, then quit. `<name>.qs.txt` is what
//! Quickshell 0.3.2 printed for `<name>.qml`: its console lines and its binding loop warnings.
//! Each test runs `qs` and the native shell (`rusty-impulse -p`) on the document: `qs` must print the
//! recorded text, the native shell the same console lines and a binding loop on the same
//! properties.
//!
//! - `lazyv`: LazyLoaders under the instances of a `Variants`, made at startup, by a model
//!   assignment and inside a LazyLoader loaded later.
//! - `lazyb`: `item` read by a binding, by a function a binding calls and by a handler of the
//!   `loadingChanged` that starts the load.
//! - `lazyl`: how many times a binding that read `item` of a loading LazyLoader is evaluated
//!   and what it holds afterwards.
//! - `quit`: `Component.onDestruction` at `Qt.quit()`: of the root's children, of the root,
//!   then of what its LazyLoaders loaded.
//! - `loop`: a binding loop with no LazyLoader in it: the values the bindings end with and how
//!   many times each is evaluated. `loop.qt612.txt` is what `qml6` of Qt 6.12 prints for it
//!   (`QT_FORCE_STDERR_LOGGING=1 QT_QPA_PLATFORM=offscreen qml6 loop.qml`), the document's
//!   address written as `file:loop.qml`.
use std::path::Path;
use std::process::{Command, Stdio};

/// What a shell printed: console lines as `qml: <text>`, Quickshell's warnings of the QML
/// engine as it prints them (`WARN scene: ...` and the line naming the binding).
fn printed(out: &str) -> Vec<String> {
    out.lines()
        .filter_map(|l| {
            if let Some(t) = l.strip_prefix("console: ") {
                Some(format!("qml: {t}"))
            } else if let Some(i) = l.find(" qml: ") {
                Some(l[i + 1..].to_string())
            } else if l.trim_start().starts_with("WARN scene: ") || l.starts_with("qs:@") {
                Some(l.trim_start().to_string())
            } else {
                l.strip_prefix("Binding loop detected for property ")
                    .map(|_| l.to_string())
            }
        })
        .collect()
}

fn console(lines: &[String]) -> Vec<&str> {
    lines
        .iter()
        .filter_map(|l| l.strip_prefix("qml: "))
        .collect()
}

/// The properties a binding loop was reported on, each once, in the order of the first report.
fn loops(lines: &[String]) -> Vec<&str> {
    let mut props: Vec<&str> = Vec::new();
    for l in lines {
        let Some(i) = l.find("Binding loop detected for property \"") else {
            continue;
        };
        let p = l[i..].split('"').nth(1).unwrap();
        if !props.contains(&p) {
            props.push(p);
        }
    }
    props
}

fn run(mut c: Command) -> Vec<String> {
    let out = c
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text += &String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "{c:?} ended with {}:\n{text}",
        out.status
    );
    printed(&text)
}

/// Runs `lazy_loader/<name>.qml` in Quickshell and in the native shell. Quickshell must print
/// `<name>.qs.txt`. The native shell must print the same console lines and report a binding
/// loop on the same properties.
fn same_as_quickshell(name: &str) {
    if !testenv::require("qs") {
        return;
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return;
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/lazy_loader");
    let doc = dir.join(format!("{name}.qml"));
    let recorded: Vec<String> = std::fs::read_to_string(dir.join(format!("{name}.qs.txt")))
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect();

    let mut qs = Command::new("qs");
    qs.arg("--no-color")
        .arg("-p")
        .arg(&doc)
        .env("QS_NO_RELOAD_POPUP", "1");
    assert_eq!(run(qs), recorded, "Quickshell on {name}.qml");

    let tmp = tempfile::tempdir().unwrap();
    std::fs::copy(&doc, tmp.path().join("shell.qml")).unwrap();
    let mut native = Command::new(env!("CARGO_BIN_EXE_rusty-impulse"));
    native.arg("-p").arg(tmp.path()).env("RI_INTERP", "1");
    let native = run(native);

    assert_eq!(
        console(&native),
        console(&recorded),
        "console lines of {name}.qml"
    );
    assert_eq!(
        loops(&native),
        loops(&recorded),
        "binding loops of {name}.qml"
    );
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn lazy_loaders_of_variants_instances_load_as_in_quickshell() {
    same_as_quickshell("lazyv");
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn item_read_by_a_binding_loads_as_in_quickshell() {
    same_as_quickshell("lazyb");
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_binding_that_read_item_is_evaluated_as_many_times_as_in_quickshell() {
    same_as_quickshell("lazyl");
}

/// Both shells destroy their objects at `Qt.quit()` and `Component.onDestruction` runs: of
/// the root's children, of the root, then of what its LazyLoaders loaded.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn on_destruction_runs_at_quit_as_in_quickshell() {
    same_as_quickshell("quit");
}

/// Qt 6.12 itself, Quickshell and the native shell on a binding loop: the same console lines
/// (the values and the evaluation counts of the three bindings after each change) and the loop
/// reported on the same property. How the report reads differs, see
/// docs/quickshell-differences.md.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_binding_loop_ends_as_in_qt() {
    if !testenv::require("qml6") {
        return;
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/lazy_loader");
    let recorded = std::fs::read_to_string(dir.join("loop.qt612.txt")).unwrap();
    let mut qt = Command::new("qml6");
    qt.arg(dir.join("loop.qml"))
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .env("QT_QPA_PLATFORM", "offscreen");
    let out = qt.stdin(Stdio::null()).output().unwrap();
    assert!(out.status.success(), "qml6 ended with {}", out.status);
    let url = format!("file://{}", dir.join("loop.qml").display());
    let printed = String::from_utf8_lossy(&out.stderr).replace(&url, "file:loop.qml");
    assert_eq!(printed, recorded, "Qt 6.12 on loop.qml");

    let qt: Vec<String> = recorded.lines().map(str::to_string).collect();
    let qs: Vec<String> = std::fs::read_to_string(dir.join("loop.qs.txt"))
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(
        console(&qs),
        console(&qt),
        "Quickshell's record against Qt's"
    );
    assert_eq!(loops(&qs), loops(&qt));
    same_as_quickshell("loop");
}
