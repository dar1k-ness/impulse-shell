//! tools/install.sh binds edit mode in the user's Hyprland config the way end-4's dots bind
//! the shell: a `hl.dsp.global("rustshell:editModeToggle")` bind (Hyprland's global shortcut
//! to EditMode.qml's `editModeToggle`, no command looked up in PATH) in custom/keybinds.lua,
//! once; a bind the user already has is kept, and `--uninstall` removes only its own line.
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const BIND: &str = r#"hl.bind("SUPER + ALT + E", hl.dsp.global("rustshell:editModeToggle"), { description = "Shell: Toggle edit mode" })"#;

/// What install.sh asks of the binary it installs: it has no JavaScript interpreter and the
/// tree it is given, also where Hyprland starts it from, is the one it was compiled from.
const BINARY: &str = r#"#!/bin/sh
if [ "$1" = --version ]; then echo 'impulse-shell test'; echo 'JavaScript interpreter: none'; echo 'Directories: impulse-shell'; exit 0; fi
if [ "$1 $2 $3" = '-c impulse-shell --check-tree' ]; then
    set -- --check-tree -p "$(realpath "$HOME/.local/share/impulse-shell")"
fi
[ "$1" = --check-tree ] && [ "$2" = -p ] || exit 2
[ "$(cat "$3/shell.qml")" = '// tree a' ] || exit 1
echo "impulse-shell: $3 has the scripts this binary was compiled from"
"#;

struct Home {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Home {
    /// A HOME, a stand-in for a built `impulse-shell` with the tree it says it was compiled
    /// from (as install_rollback.rs makes them) and, when `dots`, end-4's Lua Hyprland config.
    fn new(dots: bool) -> Home {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("source/modules")).unwrap();
        std::fs::write(root.join("source/shell.qml"), "// tree a\n").unwrap();
        std::fs::write(root.join("source/modules/Bar.qml"), "// bar of a\n").unwrap();
        let bin = root.join("impulse-shell");
        std::fs::write(&bin, BINARY).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        if dots {
            std::fs::create_dir_all(root.join("home/.config/hypr/hyprland")).unwrap();
            std::fs::write(
                root.join("home/.config/hypr/hyprland.lua"),
                "require(\"hyprland.keybinds\")\n",
            )
            .unwrap();
            std::fs::write(
                root.join("home/.config/hypr/hyprland/keybinds.lua"),
                "hl.bind(\"SUPER + A\", hl.dsp.global(\"rustshell:sidebarLeftToggle\"), { description = \"Shell: Toggle left sidebar\" })\n",
            )
            .unwrap();
        }
        Home { _dir: dir, root }
    }

    fn hypr(&self) -> PathBuf {
        self.root.join("home/.config/hypr")
    }

    fn user_binds(&self) -> PathBuf {
        self.hypr().join("custom/keybinds.lua")
    }

    fn install(&self, args: &[&str]) -> String {
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/install.sh");
        let out = Command::new("bash")
            .arg(script)
            .args(args)
            .env("BIN", self.root.join("impulse-shell"))
            .env("RI_ROOT", self.root.join("source"))
            .env("PREFIX", self.root.join("prefix"))
            .env("HOME", self.root.join("home"))
            .env("XDG_STATE_HOME", self.root.join("state"))
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("XDG_DATA_HOME")
            .env_remove("NOBUILD")
            .env_remove("RI_COMMIT")
            .output()
            .expect("run install.sh");
        assert!(
            out.status.success(),
            "install.sh {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn read(&self, p: &Path) -> String {
        std::fs::read_to_string(p).unwrap_or_default()
    }
}

#[test]
fn binds_edit_mode_once_and_uninstall_removes_only_its_line() {
    let h = Home::new(true);
    std::fs::create_dir_all(h.user_binds().parent().unwrap()).unwrap();
    // The user's own binds, last line without a newline.
    let mine = "hl.bind(\"CTRL+SUPER+ALT+Slash\", hl.dsp.exec_cmd(\"xdg-open ~/.config/hypr/custom/keybinds.lua\"), {description = \"Edit user keybinds\"} )";
    std::fs::write(h.user_binds(), mine).unwrap();
    assert!(h.install(&[]).contains("bound Super+Alt+E"));
    assert!(h.root.join("prefix/bin/impulse-shell").exists());
    assert_eq!(h.read(&h.user_binds()), format!("{mine}\n{BIND}\n"));
    // Installing again (an update) adds nothing.
    h.install(&[]);
    assert_eq!(h.read(&h.user_binds()), format!("{mine}\n{BIND}\n"));
    h.install(&["--uninstall"]);
    assert!(!h.root.join("prefix/bin/impulse-shell").exists());
    assert_eq!(h.read(&h.user_binds()), format!("{mine}\n"));
}

#[test]
fn creates_the_user_keybinds_file_the_dots_source() {
    let h = Home::new(true);
    h.install(&[]);
    assert_eq!(h.read(&h.user_binds()), format!("{BIND}\n"));
}

#[test]
fn a_bind_the_user_already_has_is_kept() {
    let h = Home::new(true);
    let theirs = "hl.bind(\"SUPER + F12\", hl.dsp.global(\"rustshell:editModeToggle\"))\n";
    std::fs::create_dir_all(h.user_binds().parent().unwrap()).unwrap();
    std::fs::write(h.user_binds(), theirs).unwrap();
    h.install(&[]);
    assert_eq!(h.read(&h.user_binds()), theirs);
    // Uninstall leaves the user's own bind alone.
    h.install(&["--uninstall"]);
    assert_eq!(h.read(&h.user_binds()), theirs);
}

#[test]
fn without_a_lua_hyprland_config_nothing_is_written() {
    let h = Home::new(false);
    assert!(h.install(&[]).contains("rustshell:editModeToggle"));
    assert!(!h.hypr().exists());
}

/// The bind the script writes is this one, and it reaches EditMode.qml's GlobalShortcut.
#[test]
fn the_bind_targets_the_edit_mode_shortcut() {
    let script = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/install.sh"),
    )
    .unwrap();
    assert!(script.contains(&format!("edit_bind='{BIND}'")));
    let shortcut = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml/modules/rusty/editMode/EditMode.qml"),
    )
    .unwrap();
    assert!(
        shortcut.contains("name: \"editModeToggle\""),
        "EditMode.qml's GlobalShortcut moved"
    );
}
