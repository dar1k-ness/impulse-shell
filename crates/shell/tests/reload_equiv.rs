//! `Quickshell.reload()` against the real Quickshell 0.3.2.
//!
//! `reload/base` is the running configuration and `reload/good` the one put over it. Each test
//! runs Quickshell and the native shell (`rusty-impulse -p`) on a copy.
//!
//! - A reload that happens runs `Component.onDestruction` of the old tree in both. Quickshell
//!   completes the new tree and tells it `reloadCompleted` first; the native shell becomes a
//!   new image, so the old tree ends first (docs/quickshell-differences.md).
//! - A reload with the compositor's socket gone: Quickshell reloads inside its process and
//!   keeps its connection. The native shell would open a new one after `exec` and end there,
//!   so it looks before: the running tree stays as it is (no `Component.onDestruction`) and
//!   is told `Quickshell.reloadFailed`, twice, as Quickshell emits the signal twice for a
//!   reload that fails (`RootWrapper::reloadGraph`, src/core/rootwrapper.cpp).
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use testenv::Log;

fn dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/reload")
        .join(name)
}

fn copy_over(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_over(&e.path(), &target);
        } else {
            std::fs::copy(e.path(), target).unwrap();
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Who {
    Quickshell,
    Native,
}

impl Who {
    fn command(self) -> Command {
        match self {
            Who::Quickshell => {
                let mut c = Command::new("qs");
                c.arg("--no-color").env("QS_NO_RELOAD_POPUP", "1");
                // A reload is the one the test asks for, not one the edit of a file starts.
                c.env("QS_DISABLE_FILE_WATCHER", "1");
                c
            }
            Who::Native => {
                let mut c = Command::new(env!("CARGO_BIN_EXE_rusty-impulse"));
                c.env("RI_INTERP", "1");
                c
            }
        }
    }

    fn shell(self, conf: &Path) -> Command {
        let mut c = self.command();
        c.arg("-p").arg(conf).stdin(Stdio::null());
        c
    }

    /// Calls `function` of the IpcHandler `t` of the shell `pid`, which need not have a
    /// `shell.qml` any more.
    fn call(self, pid: u32, function: &str) {
        let out = self
            .command()
            .args(["ipc", "--pid", &pid.to_string(), "call", "t", function])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{self:?} ipc call t {function}: {out:?}"
        );
    }
}

/// Quickshell and a compositor for it and for the native shell.
fn available() -> bool {
    if !testenv::require("qs") {
        return false;
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return false;
    }
    true
}

/// The console lines of `out`, without what precedes the text: the native shell prints
/// `console: <text>`, Quickshell `DEBUG qml: <text>`.
fn console(out: &str) -> Vec<String> {
    out.lines()
        .filter_map(|l| {
            l.strip_prefix("console: ")
                .or_else(|| l.find(" qml: ").map(|i| &l[i + 6..]))
                .map(str::to_string)
        })
        .collect()
}

struct Running {
    tmp: tempfile::TempDir,
    conf: PathBuf,
    child: Child,
    log: Arc<Log>,
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// `who` on a copy of `reload/base`, once its root completed. With `link` it reaches the
/// compositor through a link of its own to the socket, at `<tmp>/wayland`.
fn running(who: Who, link: bool) -> Running {
    let tmp = tempfile::tempdir().unwrap();
    let conf = tmp.path().join("conf");
    copy_over(&dir("base"), &conf);
    let (rd, wr) = std::io::pipe().unwrap();
    let mut c = who.shell(&conf);
    if link {
        let socket = Path::new(&std::env::var_os("XDG_RUNTIME_DIR").unwrap())
            .join(std::env::var_os("WAYLAND_DISPLAY").unwrap());
        let at = tmp.path().join("wayland");
        std::os::unix::fs::symlink(socket, &at).unwrap();
        c.env("WAYLAND_DISPLAY", at);
    }
    let child = c
        .stdout(wr.try_clone().unwrap())
        .stderr(wr)
        .spawn()
        .unwrap();
    drop(c);
    let log = Log::capture(rd);
    log.wait(|l| l.contains("ready one") && l.contains("part ready"))
        .unwrap_or_else(|l| panic!("{who:?} never loaded base:\n{l}"));
    Running {
        tmp,
        conf,
        child,
        log,
    }
}

/// What `who` says from a reload onto `reload/good` until the tree answers a call made after
/// `until` came. With `socket_gone` the reload is asked for after the shell's link to the
/// compositor's socket is removed.
fn reload(who: Who, socket_gone: bool, until: &str) -> Vec<String> {
    let shell = running(who, socket_gone);
    let before = console(&shell.log.text()).len();
    copy_over(&dir("good"), &shell.conf);
    if socket_gone {
        std::fs::remove_file(shell.tmp.path().join("wayland")).unwrap();
    }
    who.call(shell.child.id(), "reload");
    let came = |l: &str| console(l).iter().skip(before).any(|c| c.starts_with(until));
    shell
        .log
        .wait(came)
        .unwrap_or_else(|l| panic!("{who:?} did not say {until}:\n{l}"));
    who.call(shell.child.id(), "alive");
    shell
        .log
        .wait(|l| console(l).last().is_some_and(|c| c == "alive"))
        .unwrap_or_else(|l| panic!("{who:?} did not answer after the reload:\n{l}"));
    let text = shell.log.text();
    eprintln!("{who:?}:\n{text}");
    console(&text).split_off(before)
}

const OLD: [&str; 2] = ["destroyed root", "destroyed part"];
const NEW: [&str; 3] = ["ready two", "part ready", "reloadCompleted"];
const ALIVE: [&str; 1] = ["alive"];

/// Quickshell: the new tree completes and hears `reloadCompleted`, then the old one ends.
fn quickshell_reloaded() -> Vec<&'static str> {
    [&NEW[..], &OLD, &ALIVE].concat()
}

/// A reload onto a configuration that loads: the handlers of the old tree run and the new
/// tree completes, in Quickshell and in the native shell.
#[test]
fn reload_ends_the_old_tree() {
    if !available() {
        return;
    }
    let native = [&OLD[..], &NEW, &ALIVE].concat();
    assert_eq!(
        reload(Who::Quickshell, false, "destroyed part"),
        quickshell_reloaded()
    );
    assert_eq!(reload(Who::Native, false, "reloadCompleted"), native);
}

/// How many times the native shell is reloaded with the socket gone
/// (docs/quickshell-differences.md has the count).
const SOCKET_GONE_RUNS: usize = 20;

/// The compositor's socket removed before the reload: Quickshell reloads on the connection it
/// has. The native shell keeps its running tree, every time: no handler of it runs, it is told
/// why twice and it answers a call.
#[test]
fn reload_with_the_socket_gone_keeps_the_running_tree() {
    if !available() {
        return;
    }
    assert_eq!(
        reload(Who::Quickshell, true, "destroyed part"),
        quickshell_reloaded()
    );
    let kept = (0..SOCKET_GONE_RUNS)
        .filter(|_| {
            let said = reload(Who::Native, true, "reloadFailed ");
            let [first, second, alive] = &said[..] else {
                return false;
            };
            first.starts_with("reloadFailed \"Cannot connect to the Wayland compositor: ")
                && first.ends_with("\\nThe running shell keeps its current code.\"")
                && second == first
                && alive == "alive"
        })
        .count();
    assert_eq!(kept, SOCKET_GONE_RUNS, "runs that kept the running tree");
}
