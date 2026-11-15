//! A home that ran the shell under its previous name is carried over from that build:
//! `tools/carry-over.sh` copies its settings, cache and icon to the new name, `--dots new` makes
//! the user's own files name the new build and `--dots old` puts them back. Nothing of the
//! previous build is changed, and the state under the engine's word, which that build reads, is
//! not copied: `tools/install.sh` moves it to the new build's name and leaves the old name as a
//! link. Each test has a home of its own.
//!
//! The previous name is put together from halves here: `tools/rename/rename.py` renames it
//! wherever it stands whole.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

const WAS: &str = concat!("rusty", "-impulse");
const WAS_ENV: &str = concat!("RUSTY", "_IMPULSE_VIRTUAL_ENV");

/// What the user's Hyprland files hold for the previous build.
fn files() -> [(&'static str, String); 3] {
    [
        (
            "execs.lua",
            format!("hl.exec_cmd(\"$HOME/.local/bin/{WAS}-shell\")\nhl.exec_cmd(\"hypridle\")\n"),
        ),
        (
            "keybinds.lua",
            format!(
                "hl.bind(\"CTRL + SUPER + R\", hl.dsp.exec_cmd(\"killall ydotool qs quickshell {WAS}; $HOME/.local/bin/{WAS}-shell\"))\n\
                 hl.bind(\"SUPER + J\", hl.dsp.exec_cmd(\"$HOME/.local/bin/{WAS} -c $qsConfig ipc call bar toggle\"))\n\
                 hl.bind(\"SUPER + I\", hl.dsp.exec_cmd(\"$HOME/.local/bin/{WAS} -p ~/.config/rustshell/{WAS}/settings.qml\"))\n\
                 hl.bind(\"SUPER + N\", hl.dsp.global(\"rustshell:sidebarRightToggle\"))\n"
            ),
        ),
        (
            "variables.lua",
            format!(
                "hl.env(\"qsConfig\", \"{WAS}\")\nhl.env(\"{WAS_ENV}\", \"~/.local/state/rustshell/.venv\")\n"
            ),
        ),
    ]
}

struct Home {
    tmp: tempfile::TempDir,
}

impl Home {
    /// A home as the previous build leaves it: its tree, its settings, its cache, its icon, the
    /// state under the engine's word and the user's files that start and call it.
    fn new() -> Home {
        let home = Home {
            tmp: tempfile::tempdir().unwrap(),
        };
        home.put(
            &format!(".config/rustshell/{WAS}/shell.qml"),
            "ShellRoot {}\n",
        );
        home.put(
            &format!(".config/{WAS}/config.json"),
            &format!("{{\"background\": {{\"wallpaperPath\": \"~/.config/rustshell/{WAS}/assets/images/default_wallpaper.png\"}}, \"bar\": {{\"bottom\": true}}}}\n"),
        );
        home.put(
            &format!(".config/{WAS}/layout.toml"),
            "[[panel]]\nedge = \"rusty\"\n",
        );
        home.put(&format!(".cache/{WAS}/notification-images/1.png"), "png");
        home.put(&format!(".local/share/icons/{WAS}.svg"), "<svg/>");
        home.put(".local/state/rustshell/user/todo.json", "[]\n");
        for (name, text) in files() {
            home.put(&format!(".config/hypr/hyprland/{name}"), &text);
        }
        home
    }

    fn path(&self) -> &Path {
        self.tmp.path()
    }

    fn put(&self, name: &str, text: &str) {
        let path = self.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.path().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
    }

    fn hypr(&self, name: &str) -> String {
        self.read(&format!(".config/hypr/hyprland/{name}"))
    }

    /// Runs the script with these arguments; a keyring that cannot be asked (status 3) is not
    /// what is looked at here.
    fn carry_over(&self, args: &[&str]) -> String {
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/carry-over.sh");
        let out = Command::new("bash")
            .arg(script)
            .args(args)
            .env_clear()
            .env("HOME", self.path())
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap();
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            matches!(out.status.code(), Some(0 | 3)),
            "carry-over.sh {args:?}: {said}"
        );
        said
    }

    /// Every file under `dir` of the home with its content.
    fn tree(&self, dir: &str) -> BTreeMap<PathBuf, Vec<u8>> {
        fn walk(at: &Path, root: &Path, found: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for entry in std::fs::read_dir(at).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(&path, root, found);
                } else {
                    found.insert(
                        path.strip_prefix(root).unwrap().to_path_buf(),
                        std::fs::read(&path).unwrap(),
                    );
                }
            }
        }
        let mut found = BTreeMap::new();
        walk(&self.path().join(dir), self.path(), &mut found);
        found
    }
}

#[test]
fn the_settings_the_cache_and_the_icon_are_copied_to_the_new_name() {
    let home = Home::new();
    let settings = home.tree(&format!(".config/{WAS}"));
    let state = home.tree(".local/state/rustshell");
    home.carry_over(&[]);
    // The copy names the new build's tree; what does not name the build is as it was.
    assert_eq!(
        home.read(".config/impulse-shell/config.json"),
        "{\"background\": {\"wallpaperPath\": \"~/.local/share/impulse-shell/assets/images/default_wallpaper.png\"}, \"bar\": {\"bottom\": true}}\n"
    );
    assert_eq!(
        home.read(".config/impulse-shell/layout.toml"),
        "[[panel]]\nedge = \"rusty\"\n"
    );
    assert_eq!(
        home.read(".cache/impulse-shell/notification-images/1.png"),
        "png"
    );
    assert_eq!(home.read(".local/share/icons/impulse-shell.svg"), "<svg/>");
    // The previous build's settings are untouched, and its state is not copied: the install
    // moves that.
    assert_eq!(home.tree(&format!(".config/{WAS}")), settings);
    assert_eq!(home.tree(".local/state/rustshell"), state);
    let state_dirs: Vec<String> = std::fs::read_dir(home.path().join(".local/state"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|name| name != "impulse-shell")
        .collect();
    assert_eq!(state_dirs, ["rustshell"]);
}

#[test]
fn a_second_run_copies_nothing_again() {
    let home = Home::new();
    home.carry_over(&[]);
    home.put(".config/impulse-shell/config.json", "{\"mine\": true}\n");
    let said = home.carry_over(&[]);
    assert!(said.contains("already carried over"), "{said}");
    assert_eq!(
        home.read(".config/impulse-shell/config.json"),
        "{\"mine\": true}\n"
    );
}

#[test]
fn the_users_files_name_the_new_build_and_the_previous_one_after_going_back() {
    let home = Home::new();
    home.carry_over(&["--dots", "new"]);
    assert_eq!(
        home.hypr("execs.lua"),
        "hl.exec_cmd(\"$HOME/.local/bin/impulse-shell-start\")\nhl.exec_cmd(\"hypridle\")\n"
    );
    assert_eq!(
        home.hypr("keybinds.lua"),
        "hl.bind(\"CTRL + SUPER + R\", hl.dsp.exec_cmd(\"killall ydotool qs quickshell impulse-shell; $HOME/.local/bin/impulse-shell-start\"))\n\
         hl.bind(\"SUPER + J\", hl.dsp.exec_cmd(\"$HOME/.local/bin/impulse-shell -c $qsConfig ipc call bar toggle\"))\n\
         hl.bind(\"SUPER + I\", hl.dsp.exec_cmd(\"$HOME/.local/bin/impulse-shell -p ~/.local/share/impulse-shell/settings.qml\"))\n\
         hl.bind(\"SUPER + N\", hl.dsp.global(\"rustshell:sidebarRightToggle\"))\n"
    );
    assert_eq!(
        home.hypr("variables.lua"),
        "hl.env(\"qsConfig\", \"impulse-shell\")\nhl.env(\"IMPULSE_SHELL_VIRTUAL_ENV\", \"~/.local/state/impulse-shell/.venv\")\n"
    );
    home.carry_over(&["--dots", "old"]);
    for (name, text) in files() {
        assert_eq!(home.hypr(name), text, "{name}");
    }
    // And forth again: what was made for the new build comes back.
    home.carry_over(&["--dots", "new"]);
    assert_eq!(
        home.hypr("execs.lua"),
        "hl.exec_cmd(\"$HOME/.local/bin/impulse-shell-start\")\nhl.exec_cmd(\"hypridle\")\n"
    );
}

#[test]
fn a_home_that_never_had_the_previous_name_is_carried_over_from_the_first_one() {
    let home = Home {
        tmp: tempfile::tempdir().unwrap(),
    };
    // (Those names too are in halves: the test of old names reads this file.)
    home.put(concat!(".config/illogical", "-impulse/config.json"), "{}\n");
    home.put(
        concat!(".local/state/quick", "shell/user/todo.json"),
        "[]\n",
    );
    home.carry_over(&[]);
    assert_eq!(home.read(".config/impulse-shell/config.json"), "{}\n");
    assert_eq!(
        home.read(".local/state/impulse-shell/user/todo.json"),
        "[]\n"
    );
}
