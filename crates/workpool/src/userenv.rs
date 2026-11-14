//! The environment the user's session started the shell with. Quickshell copies it before it
//! applies the config's pragmas (`Common::INITIAL_ENVIRONMENT`, src/launch/launch.cpp) and
//! starts every program from that copy (`setupProcessEnvironment`, src/io/processcore.cpp):
//! `//@ pragma Env`, `//@ pragma DefaultEnv` and the style variables Quickshell sets for
//! itself reach the shell alone, never a program it starts.

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

static USER: OnceLock<Vec<(OsString, OsString)>> = OnceLock::new();

/// Keeps this process's environment as the user's. The shell calls it once, before it
/// changes its own environment; the first call wins.
pub fn capture() {
    USER.get_or_init(|| std::env::vars_os().collect());
}

/// [`capture`] with `vars` as the user's environment, for a test whose process environment
/// stands for the shell's own.
pub fn capture_as(vars: Vec<(OsString, OsString)>) {
    USER.get_or_init(|| vars);
}

/// The user's environment: the captured one, or this process's when none was captured (a
/// command-line tool, which changes nothing in its own).
pub fn vars() -> Vec<(OsString, OsString)> {
    match USER.get() {
        Some(v) => v.clone(),
        None => std::env::vars_os().collect(),
    }
}

/// `name` in the user's environment.
pub fn var(name: impl AsRef<OsStr>) -> Option<OsString> {
    match USER.get() {
        Some(v) => {
            let name = name.as_ref();
            v.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
        }
        None => std::env::var_os(name),
    }
}

/// Gives `cmd` the user's environment in place of this process's. What `cmd` was told so far
/// (`env`, `env_remove`) still applies on top.
pub fn apply(cmd: &mut Command) -> &mut Command {
    let Some(user) = USER.get() else {
        return cmd;
    };
    let own: Vec<(OsString, Option<OsString>)> = cmd
        .get_envs()
        .map(|(k, v)| (k.to_owned(), v.map(OsStr::to_owned)))
        .collect();
    cmd.env_clear().envs(user.iter().map(|(k, v)| (k, v)));
    for (k, v) in own {
        match v {
            Some(v) => cmd.env(k, v),
            None => cmd.env_remove(k),
        };
    }
    cmd
}

/// The file `execvp` would run for `program` with `path` as PATH. Empty entries are skipped.
fn find(program: &OsStr, path: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(path)
        .filter(|d| !d.as_os_str().is_empty())
        .map(|d| d.join(program))
        .find(|p| {
            let Ok(c) = std::ffi::CString::new(p.as_os_str().as_bytes()) else {
                return false;
            };
            // SAFETY: `c` is a NUL-terminated path that outlives the call.
            p.is_file() && unsafe { libc::access(c.as_ptr(), libc::X_OK) } == 0
        })
}

/// `Command::new(program)` with the user's environment ([`apply`]). A bare name is looked
/// up in the user's PATH here and run by its path, under the name as `argv[0]`: std starts
/// a path with `posix_spawn`, and a name whose environment was replaced with a `fork` of
/// this process. A name PATH lacks stays as it is and fails to start.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let program = program.as_ref();
    if USER.get().is_none() {
        return Command::new(program);
    }
    let found = (!program.as_bytes().contains(&b'/'))
        .then(|| {
            let path = var("PATH").unwrap_or_else(|| "/bin:/usr/bin".into());
            find(program, &path)
        })
        .flatten();
    let mut c = match found {
        Some(file) => {
            let mut c = Command::new(file);
            c.arg0(program);
            c
        }
        None => Command::new(program),
    };
    apply(&mut c);
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(cmd: &mut Command) -> String {
        let o = cmd
            .args([
                "-c",
                "echo \"${RI_USERENV_A-unset} ${RI_USERENV_B-unset} ${RI_USERENV_C-unset}\"",
            ])
            .output()
            .unwrap();
        String::from_utf8(o.stdout).unwrap().trim().to_string()
    }

    /// One test: the captured environment is the process's for good.
    #[test]
    fn programs_get_the_captured_environment() {
        // Nothing captured: this process's environment.
        assert_eq!(
            seen(command("sh").env("RI_USERENV_B", "own")),
            "unset own unset"
        );
        assert_eq!(var("RI_USERENV_A"), None);

        let mut user: Vec<(OsString, OsString)> = std::env::vars_os().collect();
        user.push(("RI_USERENV_A".into(), "user".into()));
        user.push(("RI_USERENV_C".into(), "user".into()));
        capture_as(user);
        capture();
        assert_eq!(var("RI_USERENV_A"), Some("user".into()));
        assert!(vars().iter().any(|(k, _)| k == "RI_USERENV_C"));
        assert_eq!(seen(&mut command("sh")), "user unset user");
        // What the command was told itself stays, set before or after.
        let mut c = Command::new("sh");
        c.env("RI_USERENV_B", "own").env_remove("RI_USERENV_C");
        assert_eq!(seen(apply(&mut c)), "user own unset");
        assert_eq!(
            seen(command("sh").env("RI_USERENV_A", "own")),
            "own unset user"
        );
        // A bare name runs by its path in the user's PATH, under its name.
        let c = command("sh");
        assert!(c.get_program().as_bytes().starts_with(b"/"), "{c:?}");
        let o = command("sh").args(["-c", "echo \"$0\""]).output().unwrap();
        assert_eq!(String::from_utf8(o.stdout).unwrap().trim(), "sh");
        assert_eq!(
            command("ri-no-such-program").get_program(),
            "ri-no-such-program"
        );
        assert_eq!(command("./x").get_program(), "./x");
    }
}
