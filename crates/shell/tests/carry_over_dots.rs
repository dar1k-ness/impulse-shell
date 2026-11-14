//! `tools/carry-over.sh --dots new` makes the user's own files name the new build, and
//! `--dots old` puts them back. What is pinned here is that the commands in those files still
//! reach the shell afterwards. The old build answered Quickshell's client, so stock files say
//! `qs -c ii ipc call ...` (or `qs -c $qsConfig ipc call ...`); the new build has a socket of
//! its own that `qs` does not know. (The rewrite made such a line `qs -c rusty-impulse ipc
//! call ...`, which nothing answers.) Each test has a home of its own; the commands of its
//! files are run there with a `qs` and a `rusty-impulse` that write down what they were asked.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A home with the user's files, and the programs their commands call.
struct Home {
    tmp: tempfile::TempDir,
}

/// What the stock files of end-4's dots call the shell with.
const STOCK_CALLS: &str = "\
qs -c $qsConfig ipc call bar toggle
qs -c ii ipc call overview toggle
bash -c 'cliphist store; qs -c $qsConfig ipc call cliphistService update'
qs -p $HOME/.config/quickshell/$qsConfig/welcome.qml
qs -p ~/.config/quickshell/ii/settings.qml
";

impl Home {
    fn new() -> Home {
        let home = Home {
            tmp: tempfile::tempdir().unwrap(),
        };
        std::fs::create_dir_all(home.hypr()).unwrap();
        std::fs::create_dir_all(home.path().join("bin")).unwrap();
        // Quickshell's client: it knows the old build's config and no other.
        home.program(
            "bin/qs",
            "case \"$*\" in *rusty-impulse*) echo 'Could not find \"rusty-impulse\" config directory' >&2; exit 255 ;; esac\n\
             echo \"qs $*\" >>\"$HOME/asked\"\n",
        );
        home
    }

    fn path(&self) -> &Path {
        self.tmp.path()
    }

    fn hypr(&self) -> PathBuf {
        self.path().join(".config/hypr/hyprland")
    }

    fn program(&self, name: &str, body: &str) {
        let path = self.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, format!("#!/bin/bash\n{body}")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// The new build installed the way the PC has it: `~/.local/bin/rusty-impulse`, a
    /// directory that is not on the PATH of what Hyprland runs.
    fn install_new(&self) {
        self.program(
            ".local/bin/rusty-impulse",
            "echo \"rusty-impulse $*\" >>\"$HOME/asked\"\n",
        );
    }

    fn write(&self, name: &str, text: &str) {
        std::fs::write(self.hypr().join(name), text).unwrap();
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.hypr().join(name)).unwrap()
    }

    fn command(&self, program: &str) -> Command {
        let mut command = Command::new(program);
        command.env_clear().env("HOME", self.path()).env(
            "PATH",
            format!("{}:/usr/bin:/bin", self.path().join("bin").display()),
        );
        command
    }

    fn carry_over(&self, to: &str) {
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/carry-over.sh");
        let out = self
            .command("bash")
            .arg(script)
            .args(["--dots", to])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "carry-over.sh --dots {to}: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Runs `calls.sh` of the home the way Hyprland runs a binding's command (`qsConfig` is
    /// what `variables.lua` sets) and gives what `qs` and `rusty-impulse` were asked.
    fn asked(&self) -> Vec<String> {
        let _ = std::fs::remove_file(self.path().join("asked"));
        let variables = self.read("variables.lua");
        let config = variables.split('"').nth(3).unwrap();
        let out = self
            .command("bash")
            .arg(self.hypr().join("calls.sh"))
            .env("qsConfig", config)
            .output()
            .unwrap();
        let asked = std::fs::read_to_string(self.path().join("asked")).unwrap_or_default();
        let home = self.path().to_str().unwrap();
        let mut lines: Vec<String> = asked.lines().map(|line| line.replace(home, "~")).collect();
        lines.extend(
            String::from_utf8_lossy(&out.stderr)
                .lines()
                .map(|line| format!("stderr: {line}")),
        );
        lines
    }
}

const VARIABLES: &str = "hl.env(\"qsConfig\", \"ii\")\n";

#[test]
fn stock_calls_through_qs_reach_the_new_build_and_the_old_one_after_going_back() {
    let home = Home::new();
    home.install_new();
    home.write("variables.lua", VARIABLES);
    home.write("calls.sh", STOCK_CALLS);
    let old = [
        "qs -c ii ipc call bar toggle",
        "qs -c ii ipc call overview toggle",
        "qs -c ii ipc call cliphistService update",
        "qs -p ~/.config/quickshell/ii/welcome.qml",
        "qs -p ~/.config/quickshell/ii/settings.qml",
    ];
    assert_eq!(home.asked(), old);

    home.carry_over("new");
    assert_eq!(
        home.asked(),
        [
            "rusty-impulse -c rusty-impulse ipc call bar toggle",
            "rusty-impulse -c rusty-impulse ipc call overview toggle",
            "rusty-impulse -c rusty-impulse ipc call cliphistService update",
            "rusty-impulse -p ~/.config/rustshell/rusty-impulse/welcome.qml",
            "rusty-impulse -p ~/.config/rustshell/rusty-impulse/settings.qml",
        ]
    );
    // By its full path: ~/.local/bin is not on the PATH these commands run with.
    assert!(
        home.read("calls.sh")
            .starts_with("$HOME/.local/bin/rusty-impulse -c $qsConfig ipc call bar toggle\n")
    );

    home.carry_over("old");
    assert_eq!(home.read("calls.sh"), STOCK_CALLS);
    assert_eq!(home.read("variables.lua"), VARIABLES);
    assert_eq!(home.asked(), old);
}

#[test]
fn calls_through_the_old_binary_reach_the_new_one() {
    let home = Home::new();
    home.install_new();
    home.program(".local/bin/ii-rs", "echo \"ii-rs $*\" >>\"$HOME/asked\"\n");
    home.write("variables.lua", VARIABLES);
    // The lines of the PC's keybinds.lua, execs.lua and config.fish.
    let calls = "$HOME/.local/bin/ii-rs -c $qsConfig ipc call sidebarLeft toggle\n\
                 bash -c 'cliphist store && $HOME/.local/bin/ii-rs -c $qsConfig ipc call cliphistService update'\n\
                 ~/.local/bin/ii-rs -c ii ipc call bar toggle\n";
    home.write("calls.sh", calls);
    let old = [
        "ii-rs -c ii ipc call sidebarLeft toggle",
        "ii-rs -c ii ipc call cliphistService update",
        "ii-rs -c ii ipc call bar toggle",
    ];
    assert_eq!(home.asked(), old);

    home.carry_over("new");
    assert_eq!(
        home.asked(),
        [
            "rusty-impulse -c rusty-impulse ipc call sidebarLeft toggle",
            "rusty-impulse -c rusty-impulse ipc call cliphistService update",
            "rusty-impulse -c rusty-impulse ipc call bar toggle",
        ]
    );

    home.carry_over("old");
    assert_eq!(home.read("calls.sh"), calls);
    assert_eq!(home.asked(), old);
}

#[test]
fn a_build_that_is_not_under_the_home_is_called_by_name() {
    let home = Home::new();
    home.program(
        "bin/rusty-impulse",
        "echo \"rusty-impulse $*\" >>\"$HOME/asked\"\n",
    );
    home.write("variables.lua", VARIABLES);
    home.write("calls.sh", "qs -c ii ipc call bar toggle\n");
    home.carry_over("new");
    assert_eq!(
        home.read("calls.sh"),
        "rusty-impulse -c rusty-impulse ipc call bar toggle\n"
    );
    assert_eq!(
        home.asked(),
        ["rusty-impulse -c rusty-impulse ipc call bar toggle"]
    );
}

/// The stock fish alias `q` is typed as `q ipc call ...`.
#[test]
fn an_alias_for_the_old_client_calls_the_new_build() {
    let home = Home::new();
    home.install_new();
    home.write("variables.lua", VARIABLES);
    let fish = home.path().join(".config/fish/config.fish");
    std::fs::create_dir_all(fish.parent().unwrap()).unwrap();
    std::fs::write(&fish, "    alias q 'qs -c ii'\n").unwrap();
    home.carry_over("new");
    let alias = std::fs::read_to_string(&fish).unwrap();
    assert_eq!(
        alias,
        "    alias q '$HOME/.local/bin/rusty-impulse -c rusty-impulse'\n"
    );
    let body = alias.split('\'').nth(1).unwrap();
    home.write("calls.sh", &format!("{body} ipc call bar toggle\n"));
    assert_eq!(
        home.asked(),
        ["rusty-impulse -c rusty-impulse ipc call bar toggle"]
    );
    home.carry_over("old");
    assert_eq!(
        std::fs::read_to_string(&fish).unwrap(),
        "    alias q 'qs -c ii'\n"
    );
}

#[test]
fn what_is_quickshells_own_stays() {
    let home = Home::new();
    home.install_new();
    // The original started by its config, a tree that is not this shell's, the processes a
    // restart binding ends, and a program whose name only ends in qs.
    let stays = "qs -c ii\n\
                 hl.exec_cmd(\"qs -c ii\")\n\
                 qs -p ~/src/other/shell.qml\n\
                 qs -c caelestia ipc call drawers toggle\n\
                 killall ydotool qs quickshell\n\
                 pidof qs quickshell hyprlock\n\
                 /usr/bin/qs --version\n";
    home.write(
        "keybinds.lua",
        &format!("{stays}qs -c ii ipc call bar toggle\n"),
    );
    home.carry_over("new");
    assert_eq!(
        home.read("keybinds.lua"),
        format!("{stays}$HOME/.local/bin/rusty-impulse -c rusty-impulse ipc call bar toggle\n")
    );
}

/// The desktop widgets' layout is one file for both builds; the old one cannot read what the
/// new one saves (`edge = "rusty"`). Going back puts the file back as it was at the switch,
/// and what the new build saved stays beside it, to be the file again at the next switch.
#[test]
fn going_back_puts_the_layout_back_and_keeps_the_new_one_beside_it() {
    let home = Home::new();
    let dir = home.path().join(".config/rusty-impulse");
    let layout = dir.join("layout.toml");
    let side = dir.join("layout.toml.from-rusty-impulse");
    let old = "version = 1\n[[widget]]\nedge = \"ii\"\nx = 10\n";
    let new = "version = 1\n[[widget]]\nedge = \"rusty\"\nx = 20\n";
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(&layout, old).unwrap();

    home.carry_over("new");
    assert_eq!(std::fs::read_to_string(&layout).unwrap(), old);
    std::fs::write(&layout, new).unwrap();
    // A second run on the new build keeps what was there at the switch.
    home.carry_over("new");
    home.carry_over("old");
    assert_eq!(std::fs::read_to_string(&layout).unwrap(), old);
    assert_eq!(std::fs::read_to_string(&side).unwrap(), new);
    assert!(!dir.join("layout.toml.prev").exists());
    home.carry_over("old");
    assert_eq!(std::fs::read_to_string(&layout).unwrap(), old);

    home.carry_over("new");
    assert_eq!(std::fs::read_to_string(&layout).unwrap(), new);
    assert!(!side.exists());
    home.carry_over("old");
    assert_eq!(std::fs::read_to_string(&layout).unwrap(), old);
    assert_eq!(std::fs::read_to_string(&side).unwrap(), new);
}

/// A layout the old build moved after going back is the old build's own: the next switch keeps
/// it for the new build and leaves the new build's copy beside it.
#[test]
fn a_layout_the_old_build_changed_since_stays() {
    let home = Home::new();
    let dir = home.path().join(".config/rusty-impulse");
    let layout = dir.join("layout.toml");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(&layout, "edge = \"ii\"\nx = 1\n").unwrap();
    home.carry_over("new");
    std::fs::write(&layout, "edge = \"rusty\"\nx = 2\n").unwrap();
    home.carry_over("old");
    std::fs::write(&layout, "edge = \"ii\"\nx = 3\n").unwrap();
    home.carry_over("new");
    assert_eq!(
        std::fs::read_to_string(&layout).unwrap(),
        "edge = \"ii\"\nx = 3\n"
    );
    home.carry_over("old");
    assert_eq!(
        std::fs::read_to_string(&layout).unwrap(),
        "edge = \"ii\"\nx = 3\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("layout.toml.from-rusty-impulse")).unwrap(),
        "edge = \"rusty\"\nx = 2\n"
    );
}

/// No layout at the switch: going back takes away the one the new build wrote, which stays
/// beside it; a new build that saved nothing leaves nothing behind.
#[test]
fn a_layout_that_was_not_there_is_not_there_after_going_back() {
    let home = Home::new();
    let dir = home.path().join(".config/rusty-impulse");
    let layout = dir.join("layout.toml");
    home.carry_over("new");
    home.carry_over("old");
    assert!(!dir.exists() || std::fs::read_dir(&dir).unwrap().next().is_none());

    home.carry_over("new");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(&layout, "edge = \"rusty\"\n").unwrap();
    home.carry_over("old");
    assert!(!layout.exists());
    assert_eq!(
        std::fs::read_to_string(dir.join("layout.toml.from-rusty-impulse")).unwrap(),
        "edge = \"rusty\"\n"
    );
    home.carry_over("new");
    assert_eq!(
        std::fs::read_to_string(&layout).unwrap(),
        "edge = \"rusty\"\n"
    );
}

/// Each round in which both builds save the layout leaves the new build's earlier copy under
/// a name with the time; only the newest of those stays, and a pile left by earlier switches
/// goes at the next one.
#[test]
fn dated_copies_of_the_layout_do_not_add_up() {
    let home = Home::new();
    let dir = home.path().join(".config/rusty-impulse");
    let layout = dir.join("layout.toml");
    std::fs::create_dir_all(&dir).unwrap();
    let dated = || {
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|name| name.starts_with("layout.toml.from-rusty-impulse."))
            .collect();
        names.sort();
        names
    };
    for old in ["20000101-000001", "20000101-000002", "20000101-000003"] {
        std::fs::write(
            dir.join(format!("layout.toml.from-rusty-impulse.{old}")),
            old,
        )
        .unwrap();
    }
    std::fs::write(&layout, "edge = \"ii\"\nx = 0\n").unwrap();
    for round in 1..=4 {
        home.carry_over("new");
        assert!(dated().len() <= 1, "round {round}: {:?}", dated());
        std::fs::write(&layout, format!("edge = \"rusty\"\nx = {round}\n")).unwrap();
        home.carry_over("old");
        assert!(dated().len() <= 1, "round {round}: {:?}", dated());
        std::fs::write(&layout, format!("edge = \"ii\"\nx = {round}\n")).unwrap();
        // The names carry the time to the second, and the rounds here take less than one:
        // the copy this round left is given the time of an earlier switch, as on a PC where
        // the next switch comes later.
        for name in dated() {
            std::fs::rename(
                dir.join(&name),
                dir.join(format!(
                    "layout.toml.from-rusty-impulse.2001010{round}-120000"
                )),
            )
            .unwrap();
        }
    }
    assert_eq!(
        std::fs::read_to_string(dir.join("layout.toml.from-rusty-impulse")).unwrap(),
        "edge = \"rusty\"\nx = 4\n"
    );
    let names = dated();
    assert_eq!(names.len(), 1, "{names:?}");
    assert_eq!(
        std::fs::read_to_string(dir.join(&names[0])).unwrap(),
        "edge = \"rusty\"\nx = 3\n"
    );
}
