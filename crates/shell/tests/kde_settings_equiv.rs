//! What the shell takes from kdeglobals under a KDE platform theme is what Qt takes from it:
//! the application font, the palette's link colour, the colour scheme, the cursor flash time
//! and the icon theme. Qt has two readers of that file. Its own QKdeTheme reads it with
//! QSettings (a number that is none is 0, a colour is a list of exactly three fields);
//! plasma-integration's plugin, which a KDE desktop has, reads it with KConfig (a number or a
//! colour that is none is the key's default, a colour may be `#rrggbb`, a name or four fields).
//! Which of the two a process gets is Qt's choice by the plugins it finds, and ours by the
//! same rule.
//!
//! Every kdeglobals below is compared with Qt itself under both: `qml6` on the Wayland
//! platform (whose integration loads the desktop's theme; the offscreen one has none) prints
//! the values, each case in a process of its own with that file in its config directory. The
//! icon theme's name has no QML API, so a QGuiApplication built here prints QIcon's.

use qml::colorscheme::{self, PlatformTheme};
use qml::runtime::Runtime;

const TEST: &str = concat!(module_path!(), "::kdeglobals_is_read_as_qt_reads_it");
const CASE: &str = "KDE_SETTINGS_EQUIV_CASE";
const ICONS: &str = "KDE_SETTINGS_EQUIV_ICONS";
/// Where tools/testenv/Containerfile keeps plasma-integration's platform theme plugin.
const PLUGINS: &str = "/opt/plasma-integration/plugins";

/// `[Colors:Button] BackgroundNormal`, without which QKdeTheme reads no colour at all.
const BUTTON: &str = "[Colors:Button]\nBackgroundNormal=200,200,200\n";

/// kdeglobals with `[General] font` being `font`.
fn font(font: &str) -> String {
    format!("[General]\nfont={font}\n")
}

/// kdeglobals whose View and Window colours all are `color`.
fn colors(color: &str) -> String {
    format!(
        "{BUTTON}[Colors:View]\nForegroundLink={color}\nBackgroundNormal={color}\n\
         ForegroundNormal={color}\n[Colors:Window]\nBackgroundNormal={color}\n"
    )
}

/// kdeglobals with a link colour and `[Colors:Button] BackgroundNormal` being `button`.
fn button(button: &str) -> String {
    format!("[Colors:Button]\nBackgroundNormal={button}\n[Colors:View]\nForegroundLink=10,20,30\n")
}

/// kdeglobals with a dark View set under the color scheme name `name`.
fn scheme(name: &str) -> String {
    format!(
        "{BUTTON}[General]\nColorScheme={name}\n[Colors:View]\nBackgroundNormal=10,20,30\n\
         ForegroundNormal=250,250,250\n"
    )
}

/// A color scheme file with a dark View and Window.
const NIGHT: &str = "[Colors:View]\nBackgroundNormal=10,20,30\nForegroundNormal=250,250,250\n\
                     ForegroundLink=1,2,3\n[Colors:Window]\nBackgroundNormal=10,20,30\n\
                     ForegroundNormal=250,250,250\n";
/// A color scheme file with a light View and Window.
const DAY: &str = "[Colors:View]\nBackgroundNormal=250,250,250\nForegroundNormal=10,20,30\n\
                   ForegroundLink=4,5,6\n[Colors:Window]\nBackgroundNormal=250,250,250\n";
const NIGHT_FILE: &str = "color-schemes/Night.colors";
const PACKAGE: &str = "[KDE]\nLookAndFeelPackage=org.test.night\n";
const PACKAGE_COLORS: &str = "plasma/look-and-feel/org.test.night/contents/colors";
const NAMES_NIGHT: &str = "[kdeglobals][General]\nColorScheme=Night\n";

/// A kdeglobals and the files of the user's data directory beside it.
type Files = (String, Vec<(&'static str, &'static str)>);

/// The cases whose colours are not in kdeglobals: without `[Colors:View]` there the plugin
/// takes a color scheme file of the data directories, which here are the test's own.
fn scheme_files() -> Vec<(&'static str, Files)> {
    let named = "[General]\nColorScheme=Night\n";
    let of = |kdeglobals: &str, files: &[(&'static str, &'static str)]| {
        (kdeglobals.to_string(), files.to_vec())
    };
    vec![
        ("a named scheme file", of(named, &[(NIGHT_FILE, NIGHT)])),
        ("a named scheme without its file", of(named, &[])),
        (
            "the colours of a look-and-feel package",
            of(PACKAGE, &[(PACKAGE_COLORS, NIGHT)]),
        ),
        (
            "a scheme named by a look-and-feel package",
            of(
                PACKAGE,
                &[
                    (
                        "plasma/look-and-feel/org.test.night/contents/defaults",
                        NAMES_NIGHT,
                    ),
                    (NIGHT_FILE, NIGHT),
                ],
            ),
        ),
        (
            "a look-and-feel package and a named scheme",
            of(
                &format!("{PACKAGE}{named}"),
                &[(PACKAGE_COLORS, DAY), (NIGHT_FILE, NIGHT)],
            ),
        ),
        (
            "the default scheme file",
            of("", &[("color-schemes/BreezeLight.colors", NIGHT)]),
        ),
        (
            "a scheme named by the default look-and-feel package",
            of(
                "",
                &[
                    (
                        "plasma/look-and-feel/org.kde.breeze.desktop/contents/defaults",
                        NAMES_NIGHT,
                    ),
                    (NIGHT_FILE, NIGHT),
                ],
            ),
        ),
        (
            "a named scheme file and Window colours in kdeglobals",
            of(
                &format!("{named}[Colors:Window]\nBackgroundNormal=250,250,250\n"),
                &[(NIGHT_FILE, NIGHT)],
            ),
        ),
    ]
}

/// The cases: a name, the kdeglobals and the files of the data directory.
fn cases() -> Vec<(String, Files)> {
    let mut cases = vec![("no file content".to_string(), (String::new(), vec![]))];
    cases.extend(scheme_files().into_iter().map(|(n, f)| (n.to_string(), f)));
    let mut case =
        |name: &str, kdeglobals: String| cases.push((name.to_string(), (kdeglobals, vec![])));
    for f in [
        "DejaVu Sans,12,-1,5,700,0,0,0,0,0,0,0,0,0,0,1",
        "DejaVu Sans,12,20,5,400,1,0,0,0,0,0,0,0,0,0,1,Bold",
        "DejaVu Sans,12,-1,5,50,0,0,0,0,0",
        "DejaVu Sans,12.5",
        "DejaVu Sans,0",
        "DejaVu Sans,-3",
        "DejaVu Sans,abc",
        "DejaVu Sans,12,-1",
        "abc",
        ",12",
        "",
    ] {
        case(&format!("font {f:?}"), font(f));
    }
    for c in [
        "10,20,30",
        " 10 , 20 , 30 ",
        "0,0,0",
        "abc",
        "",
        "1.5,2.5,3.5",
        "-3,5,5",
        "300,0,0",
        "1,2",
        "10,20,30,40",
        "10,20,30,400",
        "#102030",
        "#12",
        "red",
        "invalid",
    ] {
        case(&format!("colors {c:?}"), colors(c));
    }
    for b in ["abc", "", "-3,5,5", "1.5,2.5,3.5", "1,2,3,4"] {
        case(&format!("button {b:?}"), button(b));
    }
    for s in ["BreezeDark", "Light", "DARKLIGHT", ""] {
        case(&format!("scheme {s:?}"), scheme(s));
    }
    let view = |text: &str, base: &str| {
        format!("[Colors:View]\nForegroundNormal={text}\nBackgroundNormal={base}\n")
    };
    case(
        "a dark view without the button",
        view("250,250,250", "10,20,30"),
    );
    case(
        "a dark view",
        format!("{BUTTON}{}", view("250,250,250", "10,20,30")),
    );
    case(
        "a light view",
        format!("{BUTTON}{}", view("10,20,30", "250,250,250")),
    );
    case("the button alone", BUTTON.to_string());
    // QColor::lightness of these is 127 and 128; half of the largest and smallest channel
    // is 127 for both.
    case(
        "a text one step darker",
        format!("{BUTTON}{}", view("254,0,0", "255,0,0")),
    );
    case(
        "a text one step lighter",
        format!("{BUTTON}{}", view("255,0,0", "254,0,0")),
    );
    for t in ["Adwaita", "", "abc", " Adwaita "] {
        case(
            &format!("icon theme {t:?}"),
            format!("[Icons]\nTheme={t}\n"),
        );
    }
    for r in ["250", "abc", ""] {
        case(
            &format!("blink rate {r:?}"),
            format!("[KDE]\nCursorBlinkRate={r}\n"),
        );
    }
    cases
}

/// Prints QIcon's theme names: a QGuiApplication as `qml6` is one.
const ICONS_CPP: &str = r#"#include <QGuiApplication>
#include <QIcon>
#include <cstdio>
int main(int argc, char **argv)
{
    QGuiApplication app(argc, argv);
    printf("ICONS %s|%s\n", qPrintable(QIcon::themeName()), qPrintable(QIcon::fallbackThemeName()));
    return 0;
}
"#;

/// Builds [`ICONS_CPP`] into `dir`.
fn build_icons(dir: &std::path::Path) -> std::path::PathBuf {
    let flags = std::process::Command::new("pkg-config")
        .args(["--cflags", "--libs", "Qt6Gui"])
        .output()
        .unwrap();
    assert!(flags.status.success(), "pkg-config Qt6Gui: {flags:?}");
    let (src, bin) = (dir.join("icons.cpp"), dir.join("icons"));
    std::fs::write(&src, ICONS_CPP).unwrap();
    let built = std::process::Command::new("c++")
        .arg("-fPIC")
        .arg(&src)
        .arg("-o")
        .arg(&bin)
        .args(String::from_utf8(flags.stdout).unwrap().split_whitespace())
        .output()
        .unwrap();
    assert!(built.status.success(), "c++ icons.cpp: {built:?}");
    bin
}

/// The line a Qt program in this process's environment prints after `mark`.
fn qt_line(mut program: std::process::Command, mark: &str) -> String {
    let out = program
        .env("QT_QPA_PLATFORM", "wayland")
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .output()
        .unwrap();
    let log = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    log.lines()
        .find_map(|l| Some(l.split_once(mark)?.1.to_string()))
        .unwrap_or_else(|| panic!("{program:?} printed:\n{log}"))
}

const HINTS: &str = "[Application.styleHints.colorScheme, Application.styleHints.useHoverEffects, \
                     Application.styleHints.cursorFlashTime]";

/// Font, link colour and style hints under Qt.
fn qt_values() -> String {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("values.qml");
    let source = format!(
        r#"import QtQuick
Text {{
    Component.onCompleted: {{
        const f = Application.font
        console.log("VALUES " + [f.family, f.pointSize, f.pixelSize, f.weight, f.styleName,
                                 palette.link].concat({HINTS}).join("|"))
        Qt.quit()
    }}
}}
"#
    );
    std::fs::write(&file, source).unwrap();
    let mut qml6 = std::process::Command::new("qml6");
    qml6.arg(&file);
    qt_line(qml6, "VALUES ")
}

/// The same under our engine: the font and the colour as the scene takes them, the hints as
/// QML reads them.
fn our_values() -> String {
    let f = colorscheme::system_font();
    // The generic font is Qt's "Sans Serif", kept here under the name fontconfig knows it by.
    let family = match f.family.as_str() {
        "sans-serif" => "Sans Serif",
        family => family,
    };
    // QFont has one size: the other is derived from it at 96 dpi, the pixel size as an int.
    let point_size = match f.pixel_size {
        p if p > 0 => p as f64 * 72.0 / 96.0,
        _ => f.point_size,
    };
    let [r, g, b, a] = colorscheme::link_color();
    let link = match a {
        255 => format!("#{r:02x}{g:02x}{b:02x}"),
        _ => format!("#{a:02x}{r:02x}{g:02x}{b:02x}"),
    };
    let mut rt = Runtime::new(None);
    let source = format!("import QtQuick\nItem {{ property string hints: {HINTS}.join(\"|\") }}");
    let root = rt.load_str(&source).unwrap();
    rt.settle();
    let hints = rt.get(root, "hints");
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    format!(
        "{family}|{point_size}|{}|{}|{}|{link}|{}",
        f.pixels() as i32,
        f.weight,
        f.style_name,
        hints.to_js_string()
    )
}

#[test]
fn kdeglobals_is_read_as_qt_reads_it() {
    if !testenv::require_all(&["qml6", "c++", "pkg-config"]) {
        return;
    }
    if !testenv::in_own_process(TEST) {
        if !std::path::Path::new(PLUGINS)
            .join("platformthemes")
            .is_dir()
        {
            testenv::unavailable(format_args!(
                "plasma-integration's platform theme plugin in {PLUGINS}"
            ));
            return;
        }
        let tools = tempfile::tempdir().unwrap();
        let icons = build_icons(tools.path());
        let cases = cases();
        let runs: Vec<_> = cases.iter().flat_map(|c| [(c, false), (c, true)]).collect();
        // Each run is two Qt programs starting; a few at a time.
        for runs in runs.chunks(8) {
            std::thread::scope(|s| {
                for ((name, (kdeglobals, files)), plugin) in runs {
                    let icons = &icons;
                    s.spawn(move || {
                        let home = tempfile::tempdir().unwrap();
                        let config = home.path().join(".config");
                        std::fs::create_dir_all(&config).unwrap();
                        std::fs::write(config.join("kdeglobals"), kdeglobals).unwrap();
                        for (file, text) in files {
                            let file = home.path().join(".local/share").join(file);
                            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
                            std::fs::write(file, text).unwrap();
                        }
                        let mut vars = testenv::home_vars(home.path());
                        vars.extend([
                            ("QT_QPA_PLATFORMTHEME", "kde".into()),
                            ("KDE_SESSION_VERSION", "6".into()),
                            (CASE, name.into()),
                            (ICONS, icons.into()),
                        ]);
                        let mut unset = vec!["XDG_CURRENT_DESKTOP", "KDEHOME", "QS_ICON_THEME"];
                        if *plugin {
                            vars.push(("QT_PLUGIN_PATH", PLUGINS.into()));
                            // The shell's own style (`pragma Env` of its entry points): with
                            // the plugin Qt Quick Controls would otherwise load the desktop's.
                            vars.push(("QT_QUICK_CONTROLS_STYLE", "Basic".into()));
                        } else {
                            unset.push("QT_PLUGIN_PATH");
                        }
                        testenv::run_in_own_process_env(TEST, vars, &unset);
                    });
                }
            });
        }
        return;
    }
    let case = std::env::var(CASE).unwrap();
    let theme = colorscheme::platform_theme();
    let expected = match std::env::var_os("QT_PLUGIN_PATH") {
        Some(_) => PlatformTheme::KdePlugin,
        None => PlatformTheme::Kde,
    };
    assert_eq!(theme, expected);
    assert_eq!(our_values(), qt_values(), "{case} under {theme:?}");
    let (name, fallback) = qml::icons::theme_names();
    let icons = std::process::Command::new(std::env::var_os(ICONS).unwrap());
    assert_eq!(
        format!("{name}|{fallback}"),
        qt_line(icons, "ICONS "),
        "icon theme and its fallback, {case} under {theme:?}"
    );
}
