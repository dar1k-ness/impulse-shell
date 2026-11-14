//! The colours of the Basic style follow the system's colour scheme (QQuickBasicTheme: the
//! dark set when QPlatformTheme::colorScheme is Dark, else the light one), and a text field of
//! a style module (QtQuick.Controls.FluentWinUI3) takes its text, placeholder and selection
//! colours from that palette, as its TextField.qml in Qt says (so do Basic's TextField,
//! TextArea and Label).
//!
//! Compared with Qt itself (`qml6` on the Wayland platform, whose integration loads the
//! desktop's theme; the offscreen one has none) under each platform theme and kdeglobals
//! below, each in a process of its own with that file in its config directory. The roles are
//! the 18 the Basic style sets; `accent`, `alternateBase` and `linkVisited` it leaves to the
//! application's palette.

use qml::runtime::Runtime;

const TEST: &str = concat!(module_path!(), "::style_colours_follow_the_colour_scheme");
const CASE: &str = "STYLE_PALETTE_EQUIV_CASE";

/// QT_QPA_PLATFORMTHEME ("" is none) and the kdeglobals in the config directory.
const CASES: &[(&str, &str)] = &[
    ("", ""),
    ("kde", "[General]\nColorScheme=MaterialYouDark\n"),
    ("kde", "[General]\nColorScheme=BreezeLight\n"),
    ("kde", "[KDE]\nSingleClick=false\n"),
];

const QML: &str = r#"import QtQuick
import QtQuick.Controls
import QtQuick.Controls.FluentWinUI3 as Fluent
Item {
    id: root
    property string result
    readonly property var roles: ["base", "brightText", "button", "buttonText", "dark",
        "highlight", "highlightedText", "light", "link", "mid", "midlight", "placeholderText",
        "shadow", "text", "toolTipBase", "toolTipText", "window", "windowText"]
    TextField { id: basic }
    TextArea { id: area; enabled: false }
    Label { id: label }
    Fluent.TextField { id: field }
    Fluent.TextField { id: off; enabled: false }
    function colours(f) {
        return [f.color, f.placeholderTextColor, f.selectionColor, f.selectedTextColor].join(" ")
    }
    Component.onCompleted: {
        const out = ["scheme " + Qt.styleHints.colorScheme]
        for (const g of ["active", "inactive", "disabled"])
            out.push(g + " " + roles.map(r => basic.palette[g][r]).join(" "))
        out.push("basic " + colours(basic), "area " + colours(area))
        out.push("label " + label.color + " " + label.linkColor)
        out.push("field " + colours(field), "off " + colours(off))
        result = out.join("\n")
        //QT console.log("RESULT " + result.replace(/\n/g, "|")); Qt.quit()
    }
}
"#;

fn qt_answer(dir: &std::path::Path) -> String {
    let file = dir.join("qt.qml");
    std::fs::write(&file, QML.replace("//QT ", "")).unwrap();
    let out = std::process::Command::new("qml6")
        .arg(&file)
        .env("QT_QPA_PLATFORM", "wayland")
        // The style a Quickshell configuration has (qml6 alone takes Fusion on Linux).
        .env("QT_QUICK_CONTROLS_STYLE", "Basic")
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stderr);
    log.lines()
        .find_map(|l| Some(l.split_once("RESULT ")?.1.trim().replace('|', "\n")))
        .unwrap_or_else(|| panic!("qml6 printed:\n{log}"))
}

#[test]
fn style_colours_follow_the_colour_scheme() {
    if !testenv::require("qml6") {
        return;
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("a headless sway (WAYLAND_DISPLAY)");
        return;
    }
    if !testenv::in_own_process(TEST) {
        for (n, (theme, globals)) in CASES.iter().enumerate() {
            let home = tempfile::tempdir().unwrap();
            let config = home.path().join(".config");
            std::fs::create_dir_all(&config).unwrap();
            let mut vars = testenv::home_vars(home.path());
            let mut unset = vec!["XDG_CURRENT_DESKTOP", "KDEHOME"];
            if theme.is_empty() {
                unset.extend(["QT_QPA_PLATFORMTHEME", "KDE_SESSION_VERSION"]);
            } else {
                std::fs::write(config.join("kdeglobals"), globals).unwrap();
                vars.extend([
                    ("QT_QPA_PLATFORMTHEME", (*theme).into()),
                    ("KDE_SESSION_VERSION", "6".into()),
                ]);
            }
            vars.push((CASE, n.to_string().into()));
            testenv::run_in_own_process_env(TEST, vars, &unset);
        }
        return;
    }
    let case = CASES[std::env::var(CASE).unwrap().parse::<usize>().unwrap()];
    let dir = tempfile::tempdir().unwrap();
    let qt = qt_answer(dir.path());
    // Imported QML module types resolve for a file under a config root (not `load_str`).
    std::fs::write(dir.path().join("shell.qml"), QML).unwrap();
    let mut rt = Runtime::new(Some(dir.path().to_path_buf()));
    let root = rt.load_file(&dir.path().join("shell.qml")).unwrap();
    rt.settle();
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    assert_eq!(rt.get(root, "result").to_js_string(), qt, "{case:?}");
    // Both schemes are met: a dark answer under the dark kdeglobals, a light one elsewhere.
    let dark = case.1.contains("Dark");
    assert_eq!(qt.contains("scheme 2"), dark, "{case:?}:\n{qt}");
    let placeholder = if dark { "#88c8c9cb" } else { "#88353637" };
    assert!(qt.contains(&format!(" {placeholder} ")), "{case:?}:\n{qt}");
}
