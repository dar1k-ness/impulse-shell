//! What a recolour does to config files the user or KDE owns, where KConfig's own behaviour
//! (kconfig 6.31, `KConfig::sync`) is the reference: a `kdeglobals` that is not UTF-8 keeps
//! every entry, one that cannot be read is left alone, a link (dotfiles, stow, home-manager)
//! stays a link and the file it names is written, and what a KDE program wrote while the
//! recolour waited for the file's lock is kept. Each is checked against the real thing in
//! ri-testenv: `kwriteconfig6` / `kreadconfig6` (KConfig 6.31) given the same file, and a
//! `QLockFile` of PySide6 (Qt 6.12) holding the lock.

use colorgen::kde::{self, Body, Bus, Dirs, Outcome};
use std::io::{BufRead, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// The Python with PySide6 in ri-testenv (tools/testenv/Containerfile).
const PYTHON: &str = "/opt/qv4-venv/bin/python";

struct NoBus;

impl Bus for NoBus {
    fn signal(&self, _: &str, _: &str, _: &str, _: Body) {}
    fn has_owner(&self, _: &str) -> bool {
        false
    }
    fn call(&self, _: &str, _: &str, _: &str, _: &str) {}
    fn konsole_sessions(&self) -> Vec<(String, String)> {
        Vec::new()
    }
    fn konsole_profile(&self, _: &str, _: &str) -> Option<String> {
        None
    }
    fn set_konsole_profile(&self, _: &str, _: &str, _: &str) {}
    fn ttys(&self) -> Vec<PathBuf> {
        Vec::new()
    }
    fn write_tty(&self, _: &Path, _: &[u8]) {}
    fn pywal_reload(&self, _: &Path) {}
}

fn dirs(home: &Path) -> Dirs {
    Dirs {
        config: home.join(".config"),
        data: home.join(".local/share"),
        cache: home.join(".cache"),
    }
}

fn recolor(home: &Path) {
    match kde::recolor(&dirs(home), "#3daee9", true, 5, &NoBus) {
        Outcome::Applied(_) => {}
        o => panic!("{o:?}"),
    }
}

fn has(file: &[u8], line: &[u8]) -> bool {
    file.split(|&b| b == b'\n').any(|l| l == line)
}

/// Every name in `dir`, sorted: a lock or temporary file left behind shows here.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// `kwriteconfig6` setting the colour scheme in `file`, as a recolour does: real KConfig given
/// the user's file. False when it reports that it could not.
fn kconfig_writes(home: &Path, file: &Path) -> bool {
    std::process::Command::new("kwriteconfig6")
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .arg("--file")
        .arg(file)
        .args(["--group", "General", "--key", "ColorScheme"])
        .arg("MaterialYouDark")
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success()
}

/// What KConfig (`kreadconfig6`) reads for `group`/`key` in `file`.
fn kconfig_reads(home: &Path, file: &Path, group: &str, key: &str) -> Vec<u8> {
    let out = std::process::Command::new("kreadconfig6")
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .arg("--file")
        .arg(file)
        .args(["--group", group, "--key", key])
        .output()
        .unwrap();
    assert!(out.status.success(), "kreadconfig6 {group}/{key}");
    out.stdout
}

fn have_kconfig() -> bool {
    testenv::require("kwriteconfig6") && testenv::require("kreadconfig6")
}

/// `kdeglobals` and whatever was left next to it under its name: a lock, a temporary file.
fn kdeglobals_files(config: &Path) -> Vec<String> {
    let mut all = names(config);
    all.retain(|n| n.starts_with("kdeglobals"));
    all
}

const USERS: &[u8] = b"[General]\nName=caf\xe9\nBrowserApplication=firefox.desktop\n\n\
[KDE]\nSingleClick=false\nLookAndFeelPackage=org.kde.breezedark.desktop\n";

#[test]
fn kdeglobals_that_is_not_utf8_keeps_every_entry() {
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join(".config");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(config.join("kdeglobals"), USERS).unwrap();
    recolor(home.path());
    let after = std::fs::read(config.join("kdeglobals")).unwrap();
    for line in USERS.split(|&b| b == b'\n').filter(|l| !l.is_empty()) {
        assert!(
            has(&after, line),
            "{:?} lost from {:?}",
            String::from_utf8_lossy(line),
            String::from_utf8_lossy(&after)
        );
    }
    assert!(has(&after, b"ColorScheme=MaterialYouDark"), "recoloured");
    // KConfig reads every entry of our file as it reads them in the file it wrote itself.
    if !have_kconfig() {
        return;
    }
    let theirs = home.path().join("theirs");
    std::fs::write(&theirs, USERS).unwrap();
    assert!(kconfig_writes(home.path(), &theirs));
    for (group, key) in [
        ("General", "Name"),
        ("General", "BrowserApplication"),
        ("General", "ColorScheme"),
        ("KDE", "SingleClick"),
        ("KDE", "LookAndFeelPackage"),
    ] {
        let read = |file: &Path| kconfig_reads(home.path(), file, group, key);
        assert!(!read(&theirs).is_empty(), "{group}/{key} lost by KConfig");
        assert_eq!(
            read(&config.join("kdeglobals")),
            read(&theirs),
            "{group}/{key}"
        );
    }
}

#[test]
fn kdeglobals_that_cannot_be_read_is_left_alone() {
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join(".config");
    std::fs::create_dir_all(&config).unwrap();
    let file = config.join("kdeglobals");
    std::fs::write(&file, USERS).unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read(&file).is_ok() {
        return testenv::unavailable("a user that file modes apply to (not root)");
    }
    let inode = std::os::unix::fs::MetadataExt::ino(&std::fs::metadata(&file).unwrap());
    recolor(home.path());
    let meta = std::fs::metadata(&file).unwrap();
    assert_eq!(
        std::os::unix::fs::MetadataExt::ino(&meta),
        inode,
        "the file was replaced"
    );
    assert_eq!(meta.permissions().mode() & 0o777, 0o000);
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), USERS);
    assert_eq!(kdeglobals_files(&config), ["kdeglobals"]);
    // The rest of the recolour does not depend on that file.
    assert!(
        home.path()
            .join(".local/share/color-schemes/MaterialYouDark.colors")
            .is_file()
    );
    // KConfig gives the write up too, and leaves the file.
    if !have_kconfig() {
        return;
    }
    let theirs = home.path().join("theirs");
    std::fs::write(&theirs, USERS).unwrap();
    std::fs::set_permissions(&theirs, std::fs::Permissions::from_mode(0o000)).unwrap();
    assert!(!kconfig_writes(home.path(), &theirs));
    std::fs::set_permissions(&theirs, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(std::fs::read(&theirs).unwrap(), USERS);
}

/// A file the user made read-only (to keep KDE from changing it) is not replaced: KConfig asks
/// whether it may write the file, though the rename over it would work.
#[test]
fn kdeglobals_that_is_read_only_is_left_alone() {
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join(".config");
    std::fs::create_dir_all(&config).unwrap();
    let file = config.join("kdeglobals");
    std::fs::write(&file, USERS).unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o444)).unwrap();
    if std::fs::OpenOptions::new().append(true).open(&file).is_ok() {
        return testenv::unavailable("a user that file modes apply to (not root)");
    }
    recolor(home.path());
    assert_eq!(std::fs::read(&file).unwrap(), USERS);
    assert_eq!(
        std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o444
    );
    assert_eq!(kdeglobals_files(&config), ["kdeglobals"]);
    if !have_kconfig() {
        return;
    }
    let theirs = home.path().join("theirs");
    std::fs::write(&theirs, USERS).unwrap();
    std::fs::set_permissions(&theirs, std::fs::Permissions::from_mode(0o444)).unwrap();
    assert!(!kconfig_writes(home.path(), &theirs));
    assert_eq!(std::fs::read(&theirs).unwrap(), USERS);
}

#[test]
fn linked_kdeglobals_stays_a_link_and_its_file_is_written() {
    let home = tempfile::tempdir().unwrap();
    let (config, dot) = (home.path().join(".config"), home.path().join("dotfiles"));
    std::fs::create_dir_all(&config).unwrap();
    std::fs::create_dir_all(&dot).unwrap();
    std::fs::write(dot.join("kdeglobals"), USERS).unwrap();
    let link = config.join("kdeglobals");
    std::os::unix::fs::symlink("../dotfiles/kdeglobals", &link).unwrap();
    recolor(home.path());
    assert_eq!(
        std::fs::read_link(&link).expect("still a link"),
        Path::new("../dotfiles/kdeglobals")
    );
    let after = std::fs::read(dot.join("kdeglobals")).unwrap();
    assert!(has(&after, b"SingleClick=false"), "the user's entries stay");
    assert!(has(&after, b"ColorScheme=MaterialYouDark"), "recoloured");
    assert_eq!(
        names(&dot),
        ["kdeglobals"],
        "no lock or temporary file left"
    );
    assert_eq!(kdeglobals_files(&config), ["kdeglobals"]);
    // KConfig does the same with the same link.
    if !have_kconfig() {
        return;
    }
    let (theirs, their_dot) = (home.path().join("theirs"), home.path().join("their-dot"));
    std::fs::create_dir_all(&theirs).unwrap();
    std::fs::create_dir_all(&their_dot).unwrap();
    std::fs::write(their_dot.join("kdeglobals"), USERS).unwrap();
    let their_link = theirs.join("kdeglobals");
    std::os::unix::fs::symlink("../their-dot/kdeglobals", &their_link).unwrap();
    assert!(kconfig_writes(home.path(), &their_link));
    assert_eq!(
        std::fs::read_link(&their_link).expect("KConfig keeps the link"),
        Path::new("../their-dot/kdeglobals")
    );
    assert_eq!(names(&theirs), ["kdeglobals"]);
    assert_eq!(names(&their_dot), names(&dot));
    for (group, key) in [("General", "ColorScheme"), ("KDE", "SingleClick")] {
        assert_eq!(
            kconfig_reads(home.path(), &dot.join("kdeglobals"), group, key),
            kconfig_reads(home.path(), &their_dot.join("kdeglobals"), group, key),
            "{group}/{key}"
        );
    }
}

/// KConfig 6.31 cannot write a config that is a link to a file that is not there ("Couldn't
/// write to config") and leaves the link; so does a recolour.
#[test]
fn kdeglobals_linked_to_nothing_stays_that_link() {
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join(".config");
    std::fs::create_dir_all(&config).unwrap();
    let link = config.join("kdeglobals");
    std::os::unix::fs::symlink("../dotfiles/kdeglobals", &link).unwrap();
    recolor(home.path());
    assert_eq!(
        std::fs::read_link(&link).expect("still a link"),
        Path::new("../dotfiles/kdeglobals")
    );
    assert_eq!(kdeglobals_files(&config), ["kdeglobals"]);
    assert!(!home.path().join("dotfiles").exists());
    if !have_kconfig() {
        return;
    }
    let theirs = home.path().join("theirs");
    std::fs::create_dir_all(&theirs).unwrap();
    let their_link = theirs.join("kdeglobals");
    std::os::unix::fs::symlink("../their-dot/kdeglobals", &their_link).unwrap();
    kconfig_writes(home.path(), &their_link);
    assert_eq!(
        std::fs::read_link(&their_link).expect("KConfig keeps the link"),
        Path::new("../their-dot/kdeglobals")
    );
    assert_eq!(names(&theirs), ["kdeglobals"]);
    assert!(!home.path().join("their-dot").exists());
}

/// `titlebar_opacity` is for decorations that already have the setting: kde-material-you-colors
/// 1.10.1 leaves a `klassyrc` without `[TitleBarOpacity]` as it is (titlebar_utils.py).
#[test]
fn klassyrc_without_the_opacity_section_is_left_alone() {
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join(".config");
    std::fs::create_dir_all(config.join("klassy")).unwrap();
    std::fs::create_dir_all(config.join("kde-material-you-colors")).unwrap();
    std::fs::write(
        config.join("kde-material-you-colors/config.conf"),
        "[CUSTOM]\ntitlebar_opacity = 70\ntitlebar_opacity_dark = 55\n",
    )
    .unwrap();
    let klassyrc = config.join("klassy/klassyrc");
    std::fs::write(&klassyrc, "[Windeco]\nButtonSize=Small\n").unwrap();
    recolor(home.path());
    assert_eq!(
        std::fs::read_to_string(&klassyrc).unwrap(),
        "[Windeco]\nButtonSize=Small\n"
    );
    // With the section, the opacity is written.
    std::fs::write(&klassyrc, "[TitleBarOpacity]\nActiveTitleBarOpacity=100\n").unwrap();
    recolor(home.path());
    assert_eq!(
        std::fs::read_to_string(&klassyrc).unwrap(),
        "[TitleBarOpacity]\nActiveTitleBarOpacity=55\nInactiveTitleBarOpacity=55\n"
    );
}

/// A KDE program saving `kdeglobals`: takes its lock as KConfig does (a `QLockFile` on
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

/// KConfig's order: the lock, then the file. The recolour finds `kdeglobals` locked by a KDE
/// program in the middle of a save, waits, and then works on what that program wrote.
#[test]
fn what_kde_saved_while_we_waited_for_the_lock_is_kept() {
    if !Path::new(PYTHON).exists() {
        return testenv::unavailable(format_args!("{PYTHON} (PySide6)"));
    }
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join(".config");
    std::fs::create_dir_all(&config).unwrap();
    let file = config.join("kdeglobals");
    std::fs::write(&file, "[KDE]\nSingleClick=true\n").unwrap();
    let mut kde = std::process::Command::new(PYTHON)
        .args(["-c", KDE_SAVE])
        .arg(&file)
        .arg("[KDE]\nSingleClick=false\nAnimationDurationFactor=0.5\n")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut said = std::io::BufReader::new(kde.stdout.take().unwrap()).lines();
    assert_eq!(said.next().unwrap().unwrap(), "locked");
    // The recolour reads the lock file to learn who holds it: from then on it is waiting.
    let waiting = ReadOf::watch(&config.join("kdeglobals.lock"));
    std::thread::scope(|s| {
        let ours = s.spawn(|| recolor(home.path()));
        waiting.wait();
        kde.stdin.take().unwrap().write_all(b"\n").unwrap();
        assert_eq!(said.next().unwrap().unwrap(), "done");
        ours.join().unwrap();
    });
    assert!(kde.wait().unwrap().success());
    let after = std::fs::read(&file).unwrap();
    let text = String::from_utf8_lossy(&after);
    assert!(
        has(&after, b"SingleClick=false"),
        "KDE's change lost: {text}"
    );
    assert!(has(&after, b"AnimationDurationFactor=0.5"), "{text}");
    assert!(
        has(&after, b"ColorScheme=MaterialYouDark"),
        "recoloured: {text}"
    );
    assert_eq!(
        kdeglobals_files(&config),
        ["kdeglobals"],
        "no lock or temporary file left"
    );
}
