//! `Quickshell.reload()` while the session is locked. Quickshell reloads inside one process and
//! hands the held lock object to the next generation; rusty-impulse re-executes itself, which closes the
//! Wayland connection the lock lives on. A compositor keeps a session locked whose lock client
//! is gone (ext-session-lock-v1: "If the client dies while the session is locked, the
//! compositor must not unlock the session in response"), and the new image starts unlocked, as
//! The original's `GlobalStates.screenLocked` does: a locked screen with no lock surface and nothing to
//! unlock it. So the reload waits: the lock is kept until QML releases it, the compositor
//! answers a wl_display.sync after `unlock_and_destroy`, and only then the shell re-executes.
//!
//! On the original's own lock screen Quickshell's reload unlocks the session instead: the difference is
//! deliberate, see docs/quickshell-differences.md.
//!
//! Its own sway: the test locks the session, and a failure leaves it locked.

use std::io::Write;
use std::os::fd::AsFd;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use testenv::Log;
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, delegate_noop};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1,
    zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
};

const NATIVE: &str = env!("CARGO_BIN_EXE_rusty-impulse");

/// Unlocked at start, like the original: a re-executed image holds no lock.
const SHELL: &str = r#"import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
ShellRoot {
    id: root
    property bool locked: false
    Component.onCompleted: console.log("IMAGE up")
    WlSessionLock {
        id: lock
        locked: root.locked
        surface: Component {
            WlSessionLockSurface { color: "black" }
        }
    }
    IpcHandler {
        target: "lock"
        function lock(): void { root.locked = true }
        function unlock(): void { root.locked = false }
        function reload(): void { Quickshell.reload(true) }
        function unlockAndReload(): void { root.locked = false; Quickshell.reload(true) }
        function probe(): void { console.log("PROBE " + lock.locked + " " + lock.secure) }
    }
}
"#;

fn sway() -> bool {
    testenv::require("sway")
}

#[test]
fn fails_without_sway() {
    testenv::assert_fails_when_missing(
        "reload_under_a_session_lock_waits_for_the_unlock",
        &[("PATH", Some("/nonexistent"))],
    );
}

#[test]
fn the_lock_screen_tests_fail_without_sway_and_qs() {
    for test in [
        "a_reload_under_the_original_s_lock_screen_unlocks_in_quickshell_and_waits_in_rusty_impulse",
        "the_typed_password_and_a_pam_conversation_outlive_a_held_reload",
    ] {
        testenv::assert_fails_when_missing(test, &[("PATH", Some("/nonexistent"))]);
    }
}

/// rusty-impulse running [`SHELL`] under its own sway, with its Wayland traffic logged.
struct Shell {
    child: Child,
    sway: testenv::Sway,
    dir: tempfile::TempDir,
    /// What `ipc call` runs: the binary the shell is.
    program: std::ffi::OsString,
    /// Directories the shell reads that go with it (Quickshell's `Impulse` module).
    _with: Option<tempfile::TempDir>,
    log: Arc<Log>,
}

impl Shell {
    fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("shell.qml"), SHELL).unwrap();
        let mut cmd = Command::new(NATIVE);
        cmd.env("RI_INTERP", "1");
        Self::run(cmd, dir, None)
    }

    /// `bin -p dir` under its own sway; `bin` is `rusty-impulse` or `qs`.
    fn run(mut bin: Command, dir: tempfile::TempDir, with: Option<tempfile::TempDir>) -> Self {
        let sway = testenv::Sway::start();
        let (r, w) = std::io::pipe().unwrap();
        let xdg = dir.path().join("xdg");
        // The original's Directories.qml makes this through `Impulse.Files`, which does nothing in
        // Quickshell's copy of the module: without it Config never loads there.
        std::fs::create_dir_all(xdg.join("config/rusty-impulse")).unwrap();
        let program = bin.get_program().to_owned();
        let child = sway
            .env(bin.arg("-p").arg(dir.path()))
            .env_remove("SWAYSOCK")
            .env_remove("HYPRLAND_INSTANCE_SIGNATURE")
            .env("WAYLAND_DEBUG", "1")
            .env("XDG_CONFIG_HOME", xdg.join("config"))
            .env("XDG_STATE_HOME", xdg.join("state"))
            .env("XDG_CACHE_HOME", xdg.join("cache"))
            .stdout(w.try_clone().unwrap())
            .stderr(w)
            .spawn()
            .unwrap();
        Shell {
            child,
            sway,
            dir,
            program,
            _with: with,
            log: Log::capture(r),
        }
    }

    /// rusty-impulse on the original's lock screen ([`lock_tree`]).
    fn rusty_impulse_on_the_lock_screen() -> Self {
        let dir = lock_tree();
        let mut cmd = Command::new(NATIVE);
        cmd.env("RI_INTERP", "1")
            .env("RI_LOG", "ri::keys=debug")
            .env("LOCK_TEST_PAM", dir.path().join("pam"));
        Self::run(cmd, dir, None)
    }

    /// Quickshell on the original's lock screen. The tree says `import Impulse`, which rusty-impulse implements
    /// itself: Quickshell gets the same types with methods that do nothing.
    fn qs_on_the_lock_screen() -> Self {
        let dir = lock_tree();
        let modules = tempfile::tempdir().unwrap();
        testenv::impulse_qt_module(modules.path());
        let mut cmd = Command::new("qs");
        cmd.env("QML_IMPORT_PATH", modules.path())
            .env("QT_QUICK_BACKEND", "software")
            .env("LOCK_TEST_PAM", dir.path().join("pam"));
        Self::run(cmd, dir, Some(modules))
    }

    /// wayland-backend logs `iface#id`, its discarded-message lines `iface@id`: all as `#`.
    fn text(&self) -> String {
        self.log.text().replace('@', "#")
    }

    /// Blocks until `f(log)` holds, on each line rusty-impulse prints.
    fn wait(&self, what: &str, f: impl Fn(&str) -> bool) -> String {
        match self.log.wait(|s| f(&s.replace('@', "#"))) {
            Ok(()) => self.text(),
            Err(s) => panic!(
                "{} exited before {what}:\n{}",
                self.program.display(),
                tail(&s)
            ),
        }
    }

    fn ipc(&self, func: &str) {
        self.call("lock", func);
    }

    /// `ipc call target func`; returns once the shell has run the function.
    fn call(&self, target: &str, func: &str) {
        let xdg = self.dir.path().join("xdg");
        let ok = self
            .sway
            .env(Command::new(&self.program).arg("-p").arg(self.dir.path()))
            .env("XDG_CONFIG_HOME", xdg.join("config"))
            .env("XDG_STATE_HOME", xdg.join("state"))
            .env("XDG_CACHE_HOME", xdg.join("cache"))
            .args(["ipc", "call", target, func])
            .stdout(Stdio::null())
            .status()
            .unwrap()
            .success();
        assert!(ok, "ipc call {target} {func}:\n{}", tail(&self.text()));
    }

    /// What QML reads now: the `PROBE` line printed for this call.
    fn probe(&self) -> String {
        self.probe_by(|sh| sh.ipc("probe"))
    }

    /// The `PROBE` line `ask` makes the shell print.
    fn probe_by(&self, ask: impl Fn(&Self)) -> String {
        let n = |s: &str| s.matches("PROBE ").count();
        let before = n(&self.text());
        ask(self);
        let s = self.wait("the probe", |s| n(s) > before);
        let at = s.rfind("PROBE ").unwrap();
        s[at..].lines().next().unwrap().to_string()
    }

    /// `ri-native.locked`: the pid of the shell that holds the session lock.
    fn marker(&self) -> Option<String> {
        std::fs::read_to_string(self.sway.runtime_dir().join("ri-native.locked")).ok()
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The log lines that matter here (no per-frame traffic).
fn tail(s: &str) -> String {
    let keep = [
        "session_lock",
        "session lock",
        "reload",
        "qml:",
        "IMAGE",
        "PROBE",
        "TEXT",
        "PAM",
        "error",
    ];
    let lines: Vec<&str> = s
        .lines()
        .filter(|l| keep.iter().any(|k| l.contains(k)))
        .collect();
    lines[lines.len().saturating_sub(60)..].join("\n")
}

/// How many images of the shell have started.
fn images(s: &str) -> usize {
    s.matches("IMAGE up").count()
}

/// The first request of a connection: its registry is the first object the client creates
/// (Mesa's own registries on the same connection come later, with higher ids).
const CONNECT: &str = "wl_display#1.get_registry(new id wl_registry#2)";

/// The client sent `ext_session_lock_v1.destroy`.
fn lock_destroyed(s: &str) -> bool {
    s.lines()
        .any(|l| l.contains("ext_session_lock_v1#") && l.contains(".destroy("))
}

const DEFERRED: &str = "reload: the session is locked; reloading once it is unlocked";

/// A reload asked for under a held lock: the same image keeps the lock and its surfaces (no
/// second connection, QML still reads the lock as held and secure, the marker names it); once
/// QML unlocks, the lock goes by `unlock_and_destroy`, and the next image starts unlocked with
/// no marker left behind.
#[test]
fn reload_under_a_session_lock_waits_for_the_unlock() {
    if !sway() {
        return;
    }
    let sh = Shell::start();
    sh.wait("the first image", |s| images(s) == 1);
    sh.ipc("lock");
    sh.wait("the lock", |s| s.contains("session lock: locked"));
    let pid = sh.child.id().to_string();
    assert_eq!(sh.marker().as_deref(), Some(&*pid));

    sh.ipc("reload");
    let s = sh.wait("the reload to be held or done", |s| {
        s.contains(DEFERRED) || images(s) == 2
    });
    assert!(
        s.contains(DEFERRED) && images(&s) == 1,
        "the shell re-executed under a held session lock (lock marker: {:?}):\n{}",
        sh.marker(),
        tail(&s)
    );
    assert_eq!(sh.probe(), "PROBE true true");
    let s = sh.text();
    // The same image, and its lock object is alive: created, never destroyed.
    assert!(
        images(&s) == 1 && !s.contains(".unlock_and_destroy(") && !lock_destroyed(&s),
        "the lock was given up by the reload:\n{}",
        tail(&s)
    );
    assert_eq!(sh.marker().as_deref(), Some(&*pid));

    sh.ipc("unlock");
    let s = sh.wait("the reloaded image", |s| images(s) == 2);
    // The image that held the lock released it: the next one never locks.
    assert!(
        s.contains(".unlock_and_destroy(") && s.contains("session lock: released"),
        "the lock was not released before the reload:\n{}",
        tail(&s)
    );
    assert_eq!(sh.probe(), "PROBE false false");
    assert_eq!(sh.marker(), None, "a marker without a lock");
}

/// Unlock and reload in one turn: the exec waits for the compositor's answer to the
/// wl_display.sync that follows `unlock_and_destroy` (ext-session-lock-v1: a client that goes
/// away right after unlocking "must use the wl_display.sync request to ensure the server
/// receives and processes the unlock_and_destroy request"), so sway is unlocked when the old
/// connection closes and shows the next image's windows.
#[test]
fn unlock_and_reload_in_one_turn_unlocks_first() {
    if !sway() {
        return;
    }
    let sh = Shell::start();
    sh.wait("the first image", |s| images(s) == 1);
    sh.ipc("lock");
    sh.wait("the lock", |s| s.contains("session lock: locked"));
    sh.ipc("unlockAndReload");
    let s = sh.wait("the reloaded image", |s| images(s) == 2);
    let synced = s.find(".unlock_and_destroy(").and_then(|u| {
        // The old image's log after the unlock: up to the next connection, the new image's.
        let after = &s[u..];
        let old = &after[..after.find(CONNECT)?];
        // The sync requested after the unlock, and its `done`.
        let id = old
            .split_once("wl_display#1.sync(new id wl_callback#")?
            .1
            .split(')')
            .next()?;
        old.contains(&format!("wl_callback#{id}.done("))
            .then_some(())
    });
    assert!(
        synced.is_some(),
        "the shell re-executed before the compositor answered the unlock:\n{}",
        tail(&s)
    );
    assert_eq!(sh.probe(), "PROBE false false");
    assert_eq!(sh.marker(), None, "a marker without a lock");
}

/// The QML tree with `session_lock_reload/shell.qml` as its shell: the original's lock screen alone. Its
/// `pam/probe` service asks for the password and accepts `abc`.
fn lock_tree() -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = tempfile::tempdir().unwrap();
    testenv::tree_in_quickshells_words(&root.join("qml"), dir.path());
    std::fs::copy(
        root.join("crates/shell/tests/session_lock_reload/shell.qml"),
        dir.path().join("shell.qml"),
    )
    .unwrap();
    let pam = dir.path().join("pam");
    std::fs::create_dir(&pam).unwrap();
    // pam_exec hands the answer to the command on stdin, ended by a NUL.
    let check = pam.join("check");
    std::fs::write(
        &check,
        "#!/bin/bash\nIFS= read -r -d '' answer\n[ \"$answer\" = abc ]\n",
    )
    .unwrap();
    std::fs::set_permissions(&check, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(
        pam.join("probe"),
        format!(
            "auth required pam_exec.so expose_authtok quiet {}\n",
            check.display()
        ),
    )
    .unwrap();
    dir
}

/// The compositor told the client the session is locked (`ext_session_lock_v1.locked`).
fn locked(s: &str) -> bool {
    s.lines()
        .any(|l| l.contains("ext_session_lock_v1#") && l.contains(".locked("))
}

/// A deliberate difference from Quickshell 0.3.2, on the original's own lock screen. There a reload
/// under the lock loads the next generation at once and hands it the held lock object
/// (`WlSessionLock::onReload`); the original's `GlobalStates.screenLocked` starts false in every
/// generation, so the new `WlSessionLock` releases the lock it was handed: the reload unlocks
/// the session. rusty-impulse holds the reload until the user unlocks.
#[test]
fn a_reload_under_the_original_s_lock_screen_unlocks_in_quickshell_and_waits_in_rusty_impulse() {
    if !testenv::require_all(&["sway", "qs"]) {
        return;
    }
    let qs = Shell::qs_on_the_lock_screen();
    qs.wait("Quickshell's first generation", |s| images(s) == 1);
    qs.call("lock", "activate");
    qs.wait("Quickshell's lock", locked);
    qs.call("probe", "reload");
    // Quickshell prints this once per generation, after it reloaded the objects of the last.
    let s = qs.wait("Quickshell's second generation", |s| {
        s.matches("Configuration Loaded").count() == 2
    });
    assert!(
        s.contains(".unlock_and_destroy("),
        "Quickshell kept the session locked over a reload: rusty-impulse may now do as it does\n{}",
        tail(&s)
    );
    let probe = qs.probe_by(|sh| sh.call("probe", "state"));
    assert!(probe.contains("PROBE locked=false "), "{probe}");

    let rs = Shell::rusty_impulse_on_the_lock_screen();
    rs.wait("rusty-impulse", |s| images(s) == 1);
    rs.call("lock", "activate");
    rs.wait("rusty-impulse's lock", locked);
    rs.call("probe", "reload");
    let s = rs.wait("the reload to be held or done", |s| {
        s.contains(DEFERRED) || images(s) == 2
    });
    assert!(
        s.contains(DEFERRED) && images(&s) == 1,
        "rusty-impulse re-executed under a held session lock:\n{}",
        tail(&s)
    );
    let probe = rs.probe_by(|sh| sh.call("probe", "state"));
    assert!(probe.contains("PROBE locked=true "), "{probe}");
    let s = rs.text();
    assert!(
        images(&s) == 1 && !s.contains(".unlock_and_destroy(") && !lock_destroyed(&s),
        "the lock was given up by the reload:\n{}",
        tail(&s)
    );
}

#[derive(Default)]
struct Globals {
    seat: Option<wl_seat::WlSeat>,
    vkbd: Option<ZwpVirtualKeyboardManagerV1>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for Globals {
    fn event(
        g: &mut Self,
        reg: &wl_registry::WlRegistry,
        ev: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name, interface, ..
        } = ev
        {
            match interface.as_str() {
                "wl_seat" if g.seat.is_none() => g.seat = Some(reg.bind(name, 1, qh, ())),
                "zwp_virtual_keyboard_manager_v1" => g.vkbd = Some(reg.bind(name, 1, qh, ())),
                _ => {}
            }
        }
    }
}

delegate_noop!(Globals: ignore wl_seat::WlSeat);
delegate_noop!(Globals: ZwpVirtualKeyboardManagerV1);
delegate_noop!(Globals: ZwpVirtualKeyboardV1);

/// A keyboard on the sway's seat, which has none of its own (headless).
struct Keyboard {
    queue: EventQueue<Globals>,
    globals: Globals,
    kbd: ZwpVirtualKeyboardV1,
    _keymap: std::fs::File,
}

impl Keyboard {
    fn join(sway: &testenv::Sway) -> Option<Self> {
        let socket = std::os::unix::net::UnixStream::connect(sway.display()).unwrap();
        let conn = Connection::from_socket(socket).unwrap();
        let mut queue = conn.new_event_queue();
        let qh = queue.handle();
        conn.display().get_registry(&qh, ());
        let mut globals = Globals::default();
        queue.roundtrip(&mut globals).unwrap();
        let (Some(seat), Some(vkbd)) = (globals.seat.clone(), globals.vkbd.clone()) else {
            testenv::unavailable("sway lacks zwp_virtual_keyboard_v1");
            return None;
        };
        let kbd = vkbd.create_virtual_keyboard(&seat, &qh, ());
        let ctx = xkbcommon::xkb::Context::new(xkbcommon::xkb::CONTEXT_NO_FLAGS);
        let km = xkbcommon::xkb::Keymap::new_from_names(
            &ctx,
            "",
            "",
            "us",
            "",
            None,
            xkbcommon::xkb::KEYMAP_COMPILE_NO_FLAGS,
        )
        .unwrap();
        let text = km.get_as_string(xkbcommon::xkb::KEYMAP_FORMAT_TEXT_V1);
        let mut keymap = tempfile::tempfile().unwrap();
        keymap.write_all(text.as_bytes()).unwrap();
        keymap.write_all(&[0]).unwrap();
        kbd.keymap(1, keymap.as_fd(), text.len() as u32 + 1);
        queue.roundtrip(&mut globals).unwrap();
        Some(Keyboard {
            queue,
            globals,
            kbd,
            _keymap: keymap,
        })
    }

    /// Presses and releases a, b, c (evdev codes); returns once sway has taken them.
    fn type_abc(&mut self) {
        for (time, code) in [30, 48, 46].into_iter().enumerate() {
            self.kbd.key(time as u32 * 2, code, 1);
            self.kbd.key(time as u32 * 2 + 1, code, 0);
        }
        self.queue.roundtrip(&mut self.globals).unwrap();
    }
}

/// The compositor gave the lock surface the keyboard.
fn keyboard_entered(s: &str) -> bool {
    s.lines()
        .any(|l| l.contains("wl_keyboard#") && l.contains(".enter("))
}

/// An item of the shell took active focus (`ri::keys`): the lock surface's password field,
/// the only item that asks for it. A key that arrives before has no item to go to.
fn an_item_has_focus(s: &str) -> bool {
    s.lines()
        .any(|l| l.starts_with("focus win=") && l.contains(" -> Some("))
}

/// While a reload waits for the unlock the lock screen is the one the user was using: the
/// password typed before the reload was asked for is still in the field, and a PAM
/// conversation that had asked for it takes it as its answer afterwards.
#[test]
fn the_typed_password_and_a_pam_conversation_outlive_a_held_reload() {
    if !sway() {
        return;
    }
    let sh = Shell::rusty_impulse_on_the_lock_screen();
    let Some(mut keys) = Keyboard::join(&sh.sway) else {
        return;
    };
    sh.wait("the image", |s| images(s) == 1);
    sh.call("lock", "activate");
    sh.wait("the lock, the keyboard and the field's focus", |s| {
        locked(s) && keyboard_entered(s) && an_item_has_focus(s)
    });
    keys.type_abc();
    // The original clears a typed password it got no key for in 10 s (LockContext's passwordClearTimer):
    // the rest of the test is a handful of calls.
    sh.wait("the typed password", |s| s.contains("TEXT abc"));
    sh.call("probe", "ask");
    sh.wait("the PAM question", |s| s.contains("PAM asks"));

    sh.call("probe", "reload");
    let s = sh.wait("the reload to be held or done", |s| {
        s.contains(DEFERRED) || images(s) == 2
    });
    assert!(
        s.contains(DEFERRED) && images(&s) == 1,
        "rusty-impulse re-executed under a held session lock:\n{}",
        tail(&s)
    );
    let probe = sh.probe_by(|sh| sh.call("probe", "state"));
    assert!(
        probe.contains("PROBE locked=true text=abc pam=true/true"),
        "{probe}"
    );

    sh.call("probe", "answer");
    let s = sh.wait("the PAM answer", |s| s.contains("PAM done "));
    assert!(
        s.contains("PAM done true") && images(&s) == 1 && !s.contains(".unlock_and_destroy("),
        "the conversation did not get the password typed before the reload:\n{}",
        tail(&s)
    );
}
