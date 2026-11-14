//! `Qt.openUrlExternally` falls back to `xdg-open` when the portal answers with an error, as Qt
//! does: xdg-desktop-portal without an OpenURI backend replies UnknownMethod, and the
//! overlay's "Open recordings folder" button then opened nothing.
use qml::runtime::Runtime;
use std::io::{BufRead, BufReader};
use std::process::Command;

struct Settings;

#[zbus::interface(name = "org.freedesktop.portal.Settings")]
impl Settings {
    fn read_all(&self, _ns: Vec<String>) -> u32 {
        0
    }
}

/// A session bus at the address the environment names, with a portal lacking OpenURI on it.
fn private_bus() {
    let mut c = Command::new("dbus-daemon");
    c.args(["--session", "--nofork", "--print-address=1", "--nopidfile"])
        .arg(format!(
            "--address={}",
            std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap()
        ))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let die_with_parent = || {
        // SAFETY: prctl(2) PR_SET_PDEATHSIG takes a signal number, no pointers.
        unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) };
        Ok(())
    };
    // SAFETY: the hook runs between fork and exec; it only makes the prctl(2) syscall, which
    // is async-signal-safe, and allocates nothing.
    unsafe { std::os::unix::process::CommandExt::pre_exec(&mut c, die_with_parent) };
    let mut d = c.spawn().expect("dbus-daemon");
    let mut addr = String::new();
    BufReader::new(d.stdout.take().unwrap())
        .read_line(&mut addr)
        .unwrap();
    let addr = addr.trim().to_string();
    std::mem::forget(d);
    let c = zbus::blocking::connection::Builder::address(addr.as_str())
        .unwrap()
        .name("org.freedesktop.portal.Desktop")
        .unwrap()
        .serve_at("/org/freedesktop/portal/desktop", Settings)
        .unwrap()
        .build()
        .unwrap();
    std::mem::forget(c);
}

#[test]
fn portal_without_openuri_falls_back_to_xdg_open() {
    const TEST: &str = concat!(
        module_path!(),
        "::portal_without_openuri_falls_back_to_xdg_open"
    );
    // An `xdg-open` first on PATH that reports its arguments through a FIFO, and the session
    // bus at a socket of a fresh directory (OPENURL_DIR), in a process of its own that has
    // them in its environment.
    if !testenv::in_own_process(TEST) {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        let bin = d.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let calls = d.join("xdg-open.calls");
        assert!(
            Command::new("mkfifo")
                .arg(&calls)
                .status()
                .unwrap()
                .success()
        );
        testenv::install_script(
            &bin.join("xdg-open"),
            &format!("#!/bin/bash\necho \"$@\" > '{}'\n", calls.display()),
        );
        let path = std::env::var("PATH").unwrap_or_default();
        testenv::run_in_own_process(
            TEST,
            [
                ("PATH", format!("{}:{path}", bin.display())),
                (
                    "DBUS_SESSION_BUS_ADDRESS",
                    format!("unix:path={}", d.join("bus").display()),
                ),
                ("OPENURL_DIR", d.display().to_string()),
            ],
        );
        return;
    }
    let d = std::path::PathBuf::from(std::env::var_os("OPENURL_DIR").unwrap());
    // Open for writing too, so the FIFO never reads end-of-file between calls: each read
    // blocks until the next `xdg-open` writes its line.
    let mut calls = BufReader::new(
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(d.join("xdg-open.calls"))
            .unwrap(),
    );
    private_bus();

    let mut rt = Runtime::new(None);
    let root = rt
        .load_str("import QtQuick\nItem { function go(u) { return Qt.openUrlExternally(u) } }\n")
        .unwrap();
    let mut go = |u: &str| {
        let r = rt
            .call_method(root, "go", vec![qml::value::Value::str(u)])
            .unwrap()
            .unwrap();
        assert!(r.truthy(), "{u}");
    };
    let mut wait_for = |what: &str| {
        let mut line = String::new();
        while line.strip_suffix('\n') != Some(what) {
            line.clear();
            calls.read_line(&mut line).unwrap();
        }
    };

    let dir = format!("file://{}", d.display());
    go(&dir);
    wait_for(&dir);
    go("https://example.org/b");
    wait_for("https://example.org/b");
}
