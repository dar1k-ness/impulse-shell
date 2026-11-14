//! A theme icon Qt's platform-theme chain cannot find (Quickshell's magenta missing pixmap)
//! still resolves from the themes the user configured elsewhere, and a generic icon remains.
use std::path::Path;

fn write(p: &Path, s: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, s).unwrap();
}

fn theme(root: &Path, name: &str, inherits: &str, dirs: &[&str]) {
    let mut idx = format!(
        "[Icon Theme]\nName={name}\nInherits={inherits}\nDirectories={}\n",
        dirs.join(",")
    );
    for d in dirs {
        let size: String = d
            .chars()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(char::is_ascii_digit)
            .collect();
        idx += &format!("\n[{d}]\nSize={size}\nType=Fixed\n");
    }
    write(&root.join(name).join("index.theme"), &idx);
}

/// The user's kdeglobals under `cfg` and the icon themes under `icons`.
fn themes(cfg: &Path, icons: &Path) {
    write(&cfg.join("kdeglobals"), "[Icons]\nTheme=breeze-dark\n");
    theme(
        icons,
        "breeze-dark",
        "breeze,hicolor",
        &["apps/48", "actions/22"],
    );
    theme(icons, "breeze", "hicolor", &["apps/48", "mimetypes/32"]);
    theme(icons, "hicolor", "", &["48x48/apps"]);
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"/>"#;
    for f in [
        "breeze-dark/apps/48/preferences-system.svg",
        "breeze-dark/actions/22/application-exit.svg",
        "breeze/apps/48/systemsettings.svg",
        "breeze/mimetypes/32/application-x-executable.svg",
        "hicolor/48x48/apps/org.telegram.desktop.svg",
        "hicolor/48x48/apps/preferences-system.svg",
    ] {
        write(&icons.join(f), svg);
    }
}

#[test]
fn icons_missing_from_the_platform_theme_chain_fall_back() {
    const TEST: &str = concat!(
        module_path!(),
        "::icons_missing_from_the_platform_theme_chain_fall_back"
    );
    if !testenv::in_own_process(TEST) {
        let dir = tempfile::tempdir().unwrap();
        let t = dir.path();
        let (home, cfg, share) = (t.join("home"), t.join("cfg"), t.join("share"));
        themes(&cfg, &share.join("icons"));
        // The generic platform theme (no plugin, no desktop): Qt searches only hicolor.
        let vars: [(&str, std::ffi::OsString); 5] = [
            ("QT_PLUGIN_PATH", t.join("no-plugins").into()),
            ("HOME", home.clone().into()),
            ("XDG_CONFIG_HOME", cfg.into()),
            ("XDG_DATA_HOME", home.join(".local/share").into()),
            ("XDG_DATA_DIRS", share.into()),
        ];
        testenv::run_in_own_process_env(
            TEST,
            vars,
            &[
                "QT_QPA_PLATFORMTHEME",
                "XDG_CURRENT_DESKTOP",
                "KDE_SESSION_VERSION",
                "QS_ICON_THEME",
            ],
        );
        return;
    }
    let icons = Path::new(&std::env::var_os("XDG_DATA_DIRS").unwrap()).join("icons");
    let at = |n: &str| qml::icons::lookup(n, 20);
    let file = |f: &str| Some(icons.join(f).display().to_string());
    assert_eq!(qml::icons::theme_names().0, "hicolor");
    // Qt's own chain still wins where it finds the icon.
    assert_eq!(
        at("org.telegram.desktop"),
        file("hicolor/48x48/apps/org.telegram.desktop.svg")
    );
    assert_eq!(
        at("preferences-system"),
        file("hicolor/48x48/apps/preferences-system.svg")
    );
    // Where Quickshell draws the checkerboard: the kdeglobals theme, its parents, then breeze.
    assert_eq!(
        at("application-exit"),
        file("breeze-dark/actions/22/application-exit.svg")
    );
    assert_eq!(
        at("systemsettings"),
        file("breeze/apps/48/systemsettings.svg")
    );
    // Dash-truncated names after every theme missed the full one.
    assert_eq!(
        at("application-exit-symbolic"),
        file("breeze-dark/actions/22/application-exit.svg")
    );
    assert_eq!(at("no-such-icon-anywhere"), None);
    assert_eq!(
        qml::icons::generic_icon(20, 1),
        file("breeze/mimetypes/32/application-x-executable.svg")
    );
}
