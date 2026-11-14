//! The text caret blinks at the platform theme's cursor flash time (QStyleHints::
//! cursorFlashTime): half of it on, half off, and not at all when it is 0, which is what KDE's
//! `CursorBlinkRate=0` in kdeglobals gives (QKdeTheme). The parity run pins that 0 so that a
//! shot of a focused field does not depend on the moment it is taken.
//!
//! The flash time is compared with Qt itself (`qml6` on the Wayland platform, whose integration
//! loads the desktop's theme; the offscreen one has none) for each kdeglobals below, each in a
//! process of its own with that file in its config directory, and each under both KDE themes:
//! Qt's own QKdeTheme and plasma-integration's plugin, which a KDE desktop has and which reads
//! the same key through KConfig (a value that is no integer is its default there, not 0).

use qml::runtime::Runtime;
use shell::scene;
use std::time::{Duration, Instant};

const TEST: &str = concat!(module_path!(), "::caret_blinks_at_qt_cursor_flash_time");
const RATE: &str = "CARET_BLINK_EQUIV_RATE";
/// Where tools/testenv/Containerfile keeps plasma-integration's platform theme plugin.
const PLUGINS: &str = "/opt/plasma-integration/plugins";

/// `[KDE] CursorBlinkRate` values; "" is a kdeglobals without the key.
const RATES: &[&str] = &[
    "0", "", "-3", "1", "150", "700", "5000", "abc", "700.6", "1e2", "EMPTY",
];

/// QStyleHints::cursorFlashTime under Qt in this process's environment.
fn qt_flash_time() -> i32 {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("flash.qml");
    let source = r#"import QtQuick
Item { Component.onCompleted: { console.log("FLASH " + Application.styleHints.cursorFlashTime); Qt.quit() } }
"#;
    std::fs::write(&file, source).unwrap();
    let out = std::process::Command::new("qml6")
        .arg(&file)
        .env("QT_QPA_PLATFORM", "wayland")
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stderr);
    log.lines()
        .find_map(|l| l.split_once("FLASH ")?.1.trim().parse().ok())
        .unwrap_or_else(|| panic!("qml6 printed:\n{log}"))
}

/// Whether the caret shows `ms` after its blink restarted, and whether a toggle is due.
fn caret_after(rt: &Runtime, ms: u64) -> (bool, bool) {
    let mut w = rt.world.borrow_mut();
    w.caret_reset = Instant::now().checked_sub(Duration::from_millis(ms));
    assert!(
        w.caret_reset.is_some(),
        "the clock started under {ms} ms ago"
    );
    let (shown, toggle) = scene::caret_phase(&w);
    (shown, toggle.is_some())
}

#[test]
fn caret_blinks_at_qt_cursor_flash_time() {
    if !testenv::require("qml6") {
        return;
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("a headless sway (WAYLAND_DISPLAY)");
        return;
    }
    if !testenv::in_own_process(TEST) {
        let plugin = std::path::Path::new(PLUGINS).join("platformthemes");
        if !plugin.is_dir() {
            testenv::unavailable(format_args!(
                "plasma-integration's platform theme plugin in {PLUGINS}"
            ));
            return;
        }
        for (rate, plugin) in RATES.iter().flat_map(|r| [(r, false), (r, true)]) {
            let home = tempfile::tempdir().unwrap();
            let config = home.path().join(".config");
            std::fs::create_dir_all(&config).unwrap();
            let key = match *rate {
                "" => String::new(),
                "EMPTY" => "CursorBlinkRate=\n".into(),
                r => format!("CursorBlinkRate={r}\n"),
            };
            std::fs::write(config.join("kdeglobals"), format!("[KDE]\n{key}")).unwrap();
            // QKdeTheme, as tools/parity/setup.sh selects it; the plugin where Qt finds one.
            let mut vars = testenv::home_vars(home.path());
            vars.extend([
                ("QT_QPA_PLATFORMTHEME", "kde".into()),
                ("KDE_SESSION_VERSION", "6".into()),
                (RATE, rate.into()),
            ]);
            let mut unset = vec!["XDG_CURRENT_DESKTOP", "KDEHOME"];
            if plugin {
                vars.push(("QT_PLUGIN_PATH", PLUGINS.into()));
                // The shell's own style (`pragma Env` of its entry points): with the plugin
                // Qt Quick Controls would otherwise load the desktop's.
                vars.push(("QT_QUICK_CONTROLS_STYLE", "Basic".into()));
            } else {
                unset.push("QT_PLUGIN_PATH");
            }
            testenv::run_in_own_process_env(TEST, vars, &unset);
        }
        return;
    }
    let rate = std::env::var(RATE).unwrap();
    let theme = qml::colorscheme::platform_theme();
    let plugin = std::env::var_os("QT_PLUGIN_PATH").is_some();
    assert_eq!(
        theme,
        if plugin {
            qml::colorscheme::PlatformTheme::KdePlugin
        } else {
            qml::colorscheme::PlatformTheme::Kde
        }
    );
    let qt = qt_flash_time();
    if rate == "0" {
        assert_eq!(qt, 0, "Qt 6.12 with CursorBlinkRate=0");
    }
    assert_eq!(
        qml::colorscheme::cursor_flash_time(),
        qt,
        "cursor flash time with CursorBlinkRate={rate:?} under {theme:?}"
    );
    let rt = Runtime::new(None);
    if qt < 2 {
        // No blink timer: the caret is on whenever the shot is taken.
        for ms in [0, 250, 500, 750, 1500, 60_000] {
            assert_eq!(
                caret_after(&rt, ms),
                (true, false),
                "{ms} ms, rate {rate:?}"
            );
        }
    } else {
        // The middle of each half, away from the toggle by a quarter (50 ms at the least).
        let half = (qt / 2) as u64;
        for (halves, shown) in [(0, true), (1, false), (2, true), (3, false)] {
            let ms = halves * half + half / 2;
            assert_eq!(
                caret_after(&rt, ms),
                (shown, true),
                "{ms} ms, rate {rate:?}"
            );
        }
    }
}
