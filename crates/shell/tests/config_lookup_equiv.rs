//! Which file `-c NAME` names: the native shell and the original Quickshell
//! (`locateConfigFile`, src/launch/command.cpp) must name the same one, or none, over the same
//! config directories. `kill` is asked: it looks the config up, finds no instance of it and
//! says its path. tools/install.sh relies on this lookup: it asks the binary whether
//! `rusty-impulse -c rusty-impulse` finds the rusty-impulse config. The original is `$RI_REF_QS` or `quickshell` on PATH;
//! without it the test fails (`RI_EQUIV_SKIP=1` skips knowingly). What they say and the exit
//! status are compared, not which of stdout and stderr carries it: Quickshell logs to stdout,
//! native's client writes its messages to stderr.
//!
//! Native differs on purpose in one place: a relative `XDG_CONFIG_HOME`. Quickshell reads the
//! variable as it is here, though it ignores a relative one everywhere else (QStandardPaths);
//! the XDG Base Directory Specification has it ignored, and native ignores it everywhere.

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const NATIVE: &str = env!("CARGO_BIN_EXE_rusty-impulse");
/// What this shell calls the directory Quickshell calls `quickshell`.
const OWN: &str = "rustshell";

fn reference() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("RI_REF_QS") {
        return Some(PathBuf::from(p));
    }
    let me = std::fs::canonicalize(NATIVE).ok();
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|d| d.join("quickshell"))
        .find(|p| p.is_file() && std::fs::canonicalize(p).ok() != me)
}

/// A home, a runtime directory and a starting directory of its own for both shells.
struct Dirs {
    tmp: tempfile::TempDir,
}

impl Dirs {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        for d in ["home", "run", "cwd"] {
            std::fs::create_dir(tmp.path().join(d)).unwrap();
        }
        Dirs { tmp }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.tmp.path().join(rel)
    }

    /// An empty config file at `rel`.
    fn config(&self, rel: &str) -> PathBuf {
        let p = self.path(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "").unwrap();
        // This shell reads the same directory under its own name.
        let theirs = p.ancestors().find(|a| a.ends_with("quickshell")).unwrap();
        let _ = std::os::unix::fs::symlink("quickshell", theirs.with_file_name(OWN));
        p
    }

    /// What `bin ARGS kill` with `vars` says, and its exit status.
    fn asked(&self, bin: &Path, args: &[&str], vars: &[(&str, &str)]) -> (String, Option<i32>) {
        let o = Command::new(bin)
            .arg0("qs")
            .args(args)
            .arg("kill")
            .env_clear()
            .env("HOME", self.path("home"))
            .env("XDG_RUNTIME_DIR", self.path("run"))
            .env("XDG_CONFIG_DIRS", self.path("none"))
            .env("WAYLAND_DISPLAY", "wl-test")
            .env("QT_QPA_PLATFORM", "offscreen")
            // Qt warns on stdout about a non-UTF-8 locale.
            .env("LANG", "C.UTF-8")
            .envs(vars.iter().copied())
            .current_dir(self.path("cwd"))
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let (out, err) = (
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr),
        );
        let mut said = format!("{out}{err}");
        if bin == Path::new(NATIVE) {
            said = said.replace(&format!("/{OWN}/"), "/quickshell/");
        }
        (said, o.status.code())
    }

    /// Both shells give the same answer, which names `found` (or no config, if None).
    #[track_caller]
    fn same(&self, qt: &Path, args: &[&str], vars: &[(&str, &str)], found: Option<&Path>) {
        let want = self.asked(qt, args, vars);
        let got = self.asked(Path::new(NATIVE), args, vars);
        assert_eq!(
            got, want,
            "qs {args:?} kill with {vars:?}: native (left) differs from quickshell (right)"
        );
        let said = want.0;
        match found {
            Some(file) => {
                let file = format!("No running instances for \"{}\"", file.display());
                assert!(said.contains(&file), "no `{file}` in: {said}");
            }
            None => assert!(said.contains("Could not find"), "{said}"),
        }
    }
}

#[test]
fn a_named_config_is_found_where_quickshell_finds_it() {
    let Some(qt) = reference() else {
        testenv::unavailable("quickshell is not installed (or set RI_REF_QS)");
        return;
    };
    let d = Dirs::new();
    let config_home = d.path("config").display().to_string();
    let vars = [("XDG_CONFIG_HOME", config_home.as_str())];
    let ri = ["-c", "rusty-impulse"];

    // No config at all.
    d.same(&qt, &ri, &vars, None);
    d.same(&qt, &[], &vars, None);
    // A named config, and the default one beside it.
    let named = d.config("config/quickshell/rusty-impulse/shell.qml");
    d.same(&qt, &ri, &vars, Some(&named));
    d.same(&qt, &["-c", "other"], &vars, None);
    d.same(&qt, &[], &vars, None);
    let default = d.config("config/quickshell/default/shell.qml");
    d.same(&qt, &[], &vars, Some(&default));
    d.same(&qt, &ri, &vars, Some(&named));

    // A shell.qml in the quickshell directory itself is the default config, and no named
    // config is looked up beside it.
    let own = d.config("config/quickshell/shell.qml");
    d.same(&qt, &[], &vars, Some(&own));
    d.same(&qt, &ri, &vars, None);
    // The lookup goes on in XDG_CONFIG_DIRS.
    let system = d.config("system/quickshell/rusty-impulse/shell.qml");
    let dirs = format!(
        "{}:{}",
        d.path("empty").display(),
        d.path("system").display()
    );
    let with_dirs = [vars[0], ("XDG_CONFIG_DIRS", dirs.as_str())];
    d.same(&qt, &ri, &with_dirs, Some(&system));
    std::fs::remove_file(&own).unwrap();
    d.same(&qt, &ri, &with_dirs, Some(&named));

    // Without XDG_CONFIG_HOME, or with an empty one: $HOME/.config.
    d.same(&qt, &ri, &[], None);
    let home = d.config("home/.config/quickshell/rusty-impulse/shell.qml");
    d.same(&qt, &ri, &[], Some(&home));
    d.same(&qt, &ri, &[("XDG_CONFIG_HOME", "")], Some(&home));
}

/// The one difference: Quickshell looks a config up under a relative `XDG_CONFIG_HOME`, from
/// the directory it was started in; native ignores the value, as the specification says.
#[test]
fn a_relative_config_home_is_ignored() {
    let d = Dirs::new();
    let relative = d.config("cwd/rel/quickshell/rusty-impulse/shell.qml");
    let vars = [("XDG_CONFIG_HOME", "rel")];
    let (said, code) = d.asked(Path::new(NATIVE), &["-c", "rusty-impulse"], &vars);
    assert_eq!(code, Some(255), "{said}");
    assert!(
        said.contains("Could not find \"rusty-impulse\" config directory"),
        "{said}"
    );
    let home = d.config("home/.config/quickshell/rusty-impulse/shell.qml");
    let (said, _) = d.asked(Path::new(NATIVE), &["-c", "rusty-impulse"], &vars);
    let file = format!("No running instances for \"{}\"", home.display());
    assert!(said.contains(&file), "no `{file}` in: {said}");

    let Some(qt) = reference() else {
        testenv::unavailable("quickshell is not installed (or set RI_REF_QS)");
        return;
    };
    let (said, _) = d.asked(&qt, &["-c", "rusty-impulse"], &vars);
    let file = format!("No running instances for \"{}\"", relative.display());
    assert!(
        said.contains(&file),
        "quickshell no longer reads a relative XDG_CONFIG_HOME as it is: {said}"
    );
}
