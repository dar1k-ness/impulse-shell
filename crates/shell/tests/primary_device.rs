//! rusty-impulse creates a zwp_primary_selection_device like Qt's QWaylandPrimarySelectionDeviceManagerV1:
//! Hyprland resets the seat's primary selection whenever the pointer-focused client has none,
//! so without it `wl-copy --primary foo; wl-paste -p` prints "Nothing is copied".

use std::process::{Command, Stdio};

#[test]
fn fails_without_a_compositor() {
    testenv::assert_fails_when_missing(
        "native_binds_primary_selection_device",
        &[("WAYLAND_DISPLAY", None)],
    );
}

#[test]
fn native_binds_primary_selection_device() {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return testenv::unavailable("a Wayland compositor (WAYLAND_DISPLAY)");
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("shell.qml"),
        "import Rustshell\nShellRoot {}\n",
    )
    .unwrap();
    let (r, w) = std::io::pipe().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_rusty-impulse"))
        .arg("-p")
        .arg(dir.path())
        .env("RI_INTERP", "1")
        .env("WAYLAND_DEBUG", "1")
        .stdout(Stdio::null())
        .stderr(w)
        .spawn()
        .unwrap();
    let log = testenv::Log::capture(r);
    // Err once rusty-impulse exits without it.
    let found = log.wait(|s| {
        s.lines().any(|l| {
            l.contains("-> zwp_primary_selection_device_manager_v1") && l.contains(".get_device(")
        })
    });
    let _ = child.kill();
    let _ = child.wait();
    assert!(
        found.is_ok(),
        "rusty-impulse never created a zwp_primary_selection_device"
    );
}
