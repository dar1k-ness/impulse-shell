//! `Image.status` while a picture is created, against the real Qt 6.12.
//!
//! QQuickImageBase::componentComplete loads the picture, so its status is decided before any
//! `Component.onCompleted` runs and before its Behaviors are enabled: a synchronous picture and
//! an asynchronous one whose file the pixmap cache holds are Ready there, with their size; an
//! asynchronous one that misses the cache, or has `cache: false`, is Loading and becomes Ready
//! once the event loop has turned.
//!
//! `image_status/shell.qml` makes one picture after another, each with `opacity` bound to its
//! status under a Behavior as the shell's StyledImage has it, and prints what each handler
//! sees; the last ones print what the end of a load tells, `statusChanged` and then
//! `sourceSizeChanged`. `image_status/qt612.txt` is what `qml6` of Qt 6.12.0 printed for it
//! (`QT_FORCE_STDERR_LOGGING=1 QT_QPA_PLATFORM=offscreen qml6 image_status/shell.qml`): its
//! console lines as `qml: <text>` and its exit status as `status <n>`. The test runs the real
//! `qml6`, which must print what is recorded, and the native shell (`rusty-impulse -p`), which must
//! print the same lines.
//!
//! `image_status/places/shell.qml` does the same with the widgets of the shell that depend on
//! it, in the original's tree: StyledImage, ThumbnailImage as the wallpaper selector uses it
//! (`cache: false`), and the background of WaffleLock, which starts its opening animation in
//! `onStatusChanged`. The real Quickshell runs it on Qt 6.12 with the `Impulse` module of
//! [`testenv::impulse_qt_module`]; `image_status/places/quickshell.txt` is what it printed.
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/image_status")
}

/// The console lines of `c` as `qml: <text>` (the native shell prints `console: <text>`,
/// qml6 `qml: <text>`), then how it ended: `status <n>`, or `signal <n>`.
fn run(mut c: Command) -> Vec<String> {
    let out = c.stdin(Stdio::null()).output().unwrap();
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text += &String::from_utf8_lossy(&out.stderr);
    eprintln!("{c:?}:\n{text}");
    let mut told: Vec<String> = text
        .lines()
        .filter_map(|l| {
            // Quickshell: ` DEBUG qml: <text>`.
            let t = l
                .strip_prefix("console: ")
                .or_else(|| l.strip_prefix("qml: "))
                .or_else(|| l.trim_start().strip_prefix("DEBUG qml: "))?;
            Some(format!("qml: {t}"))
        })
        .collect();
    told.push(match (out.status.code(), out.status.signal()) {
        (Some(c), _) => format!("status {c}"),
        (None, Some(s)) => format!("signal {s}"),
        (None, None) => format!("{}", out.status),
    });
    told
}

fn recorded(file: &Path) -> Vec<String> {
    std::fs::read_to_string(file)
        .unwrap_or_else(|e| panic!("{}: {e}", file.display()))
        .lines()
        .map(str::to_string)
        .collect()
}

/// The original's tree with `image_status/places/shell.qml` as its root and the two pictures it shows.
fn places() -> tempfile::TempDir {
    let tree = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml");
    let d = tempfile::tempdir().unwrap();
    testenv::tree_in_quickshells_words(&tree, d.path());
    for (from, to) in [
        ("places/shell.qml", "shell.qml"),
        ("a.png", "p.png"),
        ("b.png", "q.png"),
    ] {
        std::fs::copy(dir().join(from), d.path().join(to)).unwrap();
    }
    d
}

/// `bin` on `tree`, with the user's configuration, state and cache in `home`.
fn isolated(bin: &Path, tree: &Path, home: &Path) -> Command {
    let mut c = Command::new(bin);
    c.arg("-p").arg(tree);
    for (k, d) in [
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_STATE_HOME", "state"),
        ("XDG_CACHE_HOME", "cache"),
    ] {
        c.env(k, home.join(d));
    }
    c
}

#[test]
#[cfg_attr(
    not(feature = "interp"),
    ignore = "not the compiled QML tree; AOT-only refuses it"
)]
fn a_picture_has_its_status_when_it_is_created_as_in_qt() {
    if !testenv::require("qml6") {
        return;
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return;
    }
    let want = recorded(&dir().join("qt612.txt"));

    let mut qt = Command::new("qml6");
    qt.arg(dir().join("shell.qml"))
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .env("QT_QPA_PLATFORM", "offscreen");
    assert_eq!(run(qt), want, "qml6 of Qt 6.12");

    let mut native = Command::new(env!("CARGO_BIN_EXE_rusty-impulse"));
    native.arg("-p").arg(dir()).env("RI_INTERP", "1");
    assert_eq!(run(native), want, "the native shell");
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn the_pictures_of_the_shell_have_their_status_when_created_as_in_quickshell() {
    if !testenv::require("qs") {
        return;
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return;
    }
    let want = recorded(&dir().join("places/quickshell.txt"));
    let tree = places();
    let home = tempfile::tempdir().unwrap();
    // The tree says `import Impulse`, which the shell implements itself: Quickshell gets the
    // same types with methods that do nothing.
    let modules = tempfile::tempdir().unwrap();
    testenv::impulse_qt_module(modules.path());

    let mut qs = isolated(Path::new("qs"), tree.path(), &home.path().join("qs"));
    qs.arg("--no-color").env("QML_IMPORT_PATH", modules.path());
    assert_eq!(run(qs), want, "Quickshell on Qt 6.12");

    let bin = Path::new(env!("CARGO_BIN_EXE_rusty-impulse"));
    let mut native = isolated(bin, tree.path(), &home.path().join("native"));
    native.env("RI_INTERP", "1");
    assert_eq!(run(native), want, "the native shell");
}
