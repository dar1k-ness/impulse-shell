//! A wallpaper switch while a KDE program holds `kdeglobals`' lock: the recolour of Qt/KDE
//! apps waits for the lock on its pool thread, the shell's loop keeps running its timers and
//! handlers meanwhile, and once the lock is released `kdeglobals` holds what that program
//! saved and the new colour scheme. The run is the shell's own: services/Switchwall.qml's
//! `WallpaperSwitch`, with no spawn hook in the way.
mod event_loop;
use event_loop::Loop;
use qml::runtime::Runtime;
use std::io::{BufRead, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

fn ri() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

/// The testenv image's Python with PySide6: Qt's own `QLockFile` and `QSaveFile`.
const PYTHON: &str = "/opt/qv4-venv/bin/python";

/// A KDE program saving the file argv[1]: takes its lock as KConfig does (a `QLockFile` on
/// `<file>.lock`), says `locked`, and on a line from stdin writes argv[2] into the file through
/// a `QSaveFile`, unlocks and says `done`.
const KDE_SAVE: &str = r#"
import sys
from PySide6.QtCore import QIODevice, QLockFile, QSaveFile
lock = QLockFile(sys.argv[1] + ".lock")
assert lock.tryLock(0), lock.error()
print("locked", flush=True)
sys.stdin.readline()
out = QSaveFile(sys.argv[1])
assert out.open(QIODevice.WriteOnly)
out.write(sys.argv[2].encode())
assert out.commit()
lock.unlock()
print("done", flush=True)
"#;

/// Blocks until `file` has been opened and closed without being written, by someone else.
struct ReadOf(OwnedFd);

impl ReadOf {
    fn watch(file: &Path) -> ReadOf {
        let path = std::ffi::CString::new(file.as_os_str().as_bytes()).unwrap();
        // SAFETY: inotify_init1(2) takes only flags.
        let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC) };
        assert!(fd >= 0, "{}", std::io::Error::last_os_error());
        // SAFETY: `fd` was just opened by inotify_init1 and nothing else owns it.
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        // SAFETY: `fd` is a live inotify fd and `path` a NUL-terminated path that outlives the
        // call.
        let wd = unsafe {
            libc::inotify_add_watch(fd.as_raw_fd(), path.as_ptr(), libc::IN_CLOSE_NOWRITE)
        };
        assert!(wd >= 0, "{}", std::io::Error::last_os_error());
        ReadOf(fd)
    }

    fn wait(&self) {
        let mut buf = [0u8; 4096];
        // SAFETY: a blocking read of the inotify fd into a local buffer.
        let got = unsafe { libc::read(self.0.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
        assert!(got > 0, "{}", std::io::Error::last_os_error());
    }
}

fn has(file: &[u8], line: &[u8]) -> bool {
    file.split(|&b| b == b'\n').any(|l| l == line)
}

#[test]
fn wallpaper_switch_waits_for_kdes_lock_off_the_loop() {
    const TEST: &str = concat!(
        module_path!(),
        "::wallpaper_switch_waits_for_kdes_lock_off_the_loop"
    );
    if !Path::new(PYTHON).exists() {
        return testenv::unavailable(format_args!("{PYTHON} (PySide6)"));
    }
    if !testenv::in_own_process(TEST) {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let cfg = home.join(".config/impulse-shell");
        std::fs::create_dir_all(&cfg).unwrap();
        // No terminal is recoloured: the run would write to every terminal of this user.
        std::fs::write(
            cfg.join("config.json"),
            r#"{"appearance":{"wallpaperTheming":{"enableTerminal":false}}}"#,
        )
        .unwrap();
        std::fs::write(home.join(".config/kdeglobals"), "[KDE]\nSingleClick=true\n").unwrap();
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join(
                "../colorgen/tests/switchwall/fixture/home/Pictures/Wallpapers/syn_small.png",
            ),
            home.join("wall.png"),
        )
        .unwrap();
        std::fs::create_dir_all(home.join("run")).unwrap();
        let mut vars = testenv::home_vars(home);
        vars.push(("XDG_RUNTIME_DIR", home.join("run").into()));
        // No session bus and no compositor of whoever runs the test is told of the change.
        testenv::run_in_own_process_env(
            TEST,
            vars,
            &[
                "DBUS_SESSION_BUS_ADDRESS",
                "HYPRLAND_INSTANCE_SIGNATURE",
                "WAYLAND_DISPLAY",
                "DISPLAY",
            ],
        );
        return;
    }
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let kdeglobals = home.join(".config/kdeglobals");
    let mut kde = std::process::Command::new(PYTHON)
        .args(["-c", KDE_SAVE])
        .arg(&kdeglobals)
        .arg("[KDE]\nSingleClick=false\nAnimationDurationFactor=0.5\n")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut said = std::io::BufReader::new(kde.stdout.take().unwrap()).lines();
    assert_eq!(said.next().unwrap().unwrap(), "locked");
    // The recolour reads the lock file to learn who holds it: from then on it is waiting.
    let waiting = ReadOf::watch(&home.join(".config/kdeglobals.lock"));

    let r = ri();
    let mut rt = Runtime::new(Some(r.clone()));
    let dirs: Vec<PathBuf> = [
        "modules/common",
        "modules/common/functions",
        "modules/common/utils",
        "services",
        "",
    ]
    .iter()
    .map(|d| r.join(d))
    .collect();
    rt.load_singletons_lazy(&dirs);
    let root = rt
        .load_str(
            "import QtQuick\nimport ri.services\nItem {\n    property int beats: 0\n    \
             property bool busy: Switchwall.busy\n    \
             Timer { interval: 100; repeat: true; running: true; onTriggered: parent.beats++ }\n    \
             function switchTo(image) { Switchwall.run({ image: image, mode: \"dark\" }) }\n}\n",
        )
        .unwrap_or_else(|e| panic!("{e}"));
    let read = |rt: &mut Runtime, k: &str| rt.world.borrow_mut().read(root, k).unwrap();
    let mut lp = Loop::new(&mut rt);
    lp.advance(&mut rt, 300.0);
    assert!(!read(&mut rt, "busy").truthy(), "a run before the switch");

    let image = home.join("wall.png").display().to_string();
    rt.call_method(root, "switchTo", vec![qml::value::Value::str(&image)])
        .unwrap()
        .unwrap();
    lp.flush(&mut rt);
    waiting.wait();

    // The run is held by the lock, on its pool thread. The loop takes ten more turns, each
    // 100 ms of its clock later: the timer fires in every one and the queue's state is read.
    let before = read(&mut rt, "beats")
        .to_js_string()
        .parse::<i32>()
        .unwrap();
    for _ in 0..10 {
        lp.now += 100.0;
        lp.flush(&mut rt);
    }
    let beats = read(&mut rt, "beats")
        .to_js_string()
        .parse::<i32>()
        .unwrap();
    assert_eq!(beats - before, 10, "the loop's timer while the run waits");
    assert!(read(&mut rt, "busy").truthy(), "the run is still going");
    assert_eq!(
        std::fs::read_to_string(&kdeglobals).unwrap(),
        "[KDE]\nSingleClick=true\n",
        "nothing is written while KDE holds the lock"
    );

    kde.stdin.take().unwrap().write_all(b"\n").unwrap();
    assert_eq!(said.next().unwrap().unwrap(), "done");
    assert!(kde.wait().unwrap().success());
    lp.block_until(&mut rt, |rt| {
        !rt.world.borrow_mut().read(root, "busy").unwrap().truthy()
    });

    let after = std::fs::read(&kdeglobals).unwrap();
    let text = String::from_utf8_lossy(&after);
    assert!(
        has(&after, b"SingleClick=false"),
        "KDE's change lost: {text}"
    );
    assert!(has(&after, b"AnimationDurationFactor=0.5"), "{text}");
    // Dark or light is read back from dconf, which no session bus here stores it in.
    assert!(
        has(&after, b"ColorScheme=MaterialYouDark") || has(&after, b"ColorScheme=MaterialYouLight"),
        "recoloured: {text}"
    );
    assert!(
        !home.join(".config/kdeglobals.lock").exists(),
        "the lock is released"
    );
    let config = std::fs::read_to_string(home.join(".config/impulse-shell/config.json"));
    let config: serde_json::Value = serde_json::from_str(&config.unwrap()).unwrap();
    assert_eq!(config["background"]["wallpaperPath"], image);
}
