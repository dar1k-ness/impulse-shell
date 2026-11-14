//! Every line of the shell's stderr reaches the descriptor whole, in one write(2), whoever
//! prints it: the shell (elog), or a C library in its process (libwayland's WAYLAND_DEBUG).
//! Threads print there at once, and a line written in pieces gets another thread's line between
//! two of them: a log no test and no person can read line by line.
//!
//! Checked on a headless sway, with a socket that keeps each write apart as the shell's stderr.

use std::io::Read;
use std::os::fd::{FromRawFd, OwnedFd};
use std::process::{Command, Stdio};

#[test]
fn fails_without_sway() {
    testenv::assert_fails_when_missing(
        "a_line_of_any_length_is_one_write",
        &[("PATH", Some("/nonexistent"))],
    );
}

/// A connected pair of sockets on which every write(2) is read as one record, and a read
/// returns 0 once the other end is closed.
fn record_sockets() -> (OwnedFd, OwnedFd) {
    let mut fds = [0; 2];
    // SAFETY: socketpair(2) fills the two descriptors `fds` has room for.
    let r = unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_SEQPACKET, 0, fds.as_mut_ptr()) };
    assert_eq!(r, 0, "socketpair: {}", std::io::Error::last_os_error());
    // SAFETY: a descriptor socketpair(2) just opened, owned by nothing else.
    fds.map(|fd| unsafe { OwnedFd::from_raw_fd(fd) }).into()
}

#[test]
fn a_line_of_any_length_is_one_write() {
    if !testenv::require("sway") {
        return;
    }
    // glibc writes an unbuffered stream 128 bytes at a time, and one with the line buffer it
    // allocates itself 4096 bytes at a time: the request that names this layer is longer in
    // WAYLAND_DEBUG's log, and as long as a Wayland message (4096 bytes) lets a name be.
    let namespace = "log-lines:".to_owned() + &"n".repeat(4000);
    let sway = testenv::Sway::start();
    let dir = tempfile::tempdir().unwrap();
    let qml = format!(
        r#"import QtQuick
import Rustshell
import Rustshell.Wayland
ShellRoot {{
    PanelWindow {{
        WlrLayershell.namespace: "{namespace}"
        anchors {{ top: true; left: true }}
        implicitWidth: 64; implicitHeight: 32
        Component.onCompleted: console.log("a line of the shell's own, as long:", "{namespace}")
    }}
}}
"#
    );
    std::fs::write(dir.path().join("shell.qml"), qml).unwrap();
    let (ours, theirs) = record_sockets();
    let xdg = dir.path().join("xdg");
    let mut child = Command::new(env!("CARGO_BIN_EXE_rusty-impulse"))
        .arg("-p")
        .arg(dir.path())
        .env("WAYLAND_DISPLAY", sway.display())
        .env("XDG_RUNTIME_DIR", sway.runtime_dir())
        .env_remove("SWAYSOCK")
        .env_remove("HYPRLAND_INSTANCE_SIGNATURE")
        .env("RI_INTERP", "1")
        .env("WAYLAND_DEBUG", "1")
        .env("XDG_CONFIG_HOME", xdg.join("config"))
        .env("XDG_STATE_HOME", xdg.join("state"))
        .env("XDG_CACHE_HOME", xdg.join("cache"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(theirs)
        .spawn()
        .unwrap();
    let mut log = std::fs::File::from(ours);
    let mut buf = vec![0; 1 << 16];
    let (mut layer, mut own) = (false, false);
    let mut seen = String::new();
    while !(layer && own) {
        let n = log.read(&mut buf).unwrap();
        let write = String::from_utf8_lossy(&buf[..n]);
        assert!(
            n > 0,
            "rusty-impulse exited before it logged its layer and its line:\n{seen}"
        );
        seen += &write;
        assert!(
            write.ends_with('\n'),
            "a write that ends within a line: {write:?}"
        );
        let whole = |start: &str| {
            write
                .lines()
                .any(|l| l.contains(start) && l.contains(&namespace))
        };
        layer |= whole(".get_layer_surface(");
        own |= whole("a line of the shell's own, as long:");
    }
    child.kill().unwrap();
    child.wait().unwrap();
}
