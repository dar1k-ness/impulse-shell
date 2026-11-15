//! The one command (`install.sh` in the repository's root) in a home that runs the shell under
//! its previous name, with that build's own rollback watch on: the owner's PC. The command was
//! written for a home that runs Quickshell: it stopped `qs`, and neither the previous build's
//! shell nor its watch, so both shells ran, and the watch would have started the previous build
//! again at the next login; the Hyprland files then started the shell by a start script that
//! nothing had put there. Pinned here: the previous shell and its watch are stopped, the new
//! shell runs with the settings carried over, its start script is there; and a new shell that
//! does not come up leaves the home as it was, the previous shell and its watch running again.
//!
//! Each test has a home of its own; `one_command_previous/run.sh` lays it out and runs the
//! repository's install.sh there.

use std::path::Path;
use std::process::Command;

/// The shell's previous name, in halves: `tools/rename/rename.py` renames it wherever it stands
/// whole.
const WAS: &str = concat!("rusty", "-impulse");

/// Runs install.sh in a new home (`case`: see run.sh). `None` when a tool is missing.
fn install(case: &str) -> Option<String> {
    if !testenv::require_all(&["unshare", "jq", "pgrep", "setsid"]) {
        return None;
    }
    let home = tempfile::tempdir().unwrap();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = Command::new("bash")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/one_command_previous/run.sh"))
        .arg(&repo)
        .arg(case)
        .env("HOME", home.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if !stdout.contains("install: status ") {
        testenv::unavailable(format!(
            "no PID namespace (docker run --security-opt seccomp=unconfined --security-opt systempaths=unconfined): {stdout}{}",
            String::from_utf8_lossy(&out.stderr)
        ));
        return None;
    }
    Some(stdout)
}

fn has_line(out: &str, line: &str) -> bool {
    out.lines().any(|l| l == line)
}

/// What the files that started the previous build read once they start the new one.
fn new_files(out: &str) {
    assert!(
        has_line(
            out,
            "execs.lua: hl.exec_cmd(\"$HOME/.local/bin/impulse-shell-start\")"
        ),
        "{out}"
    );
    assert!(
        has_line(
            out,
            "start script: exec \"$HOME/.local/bin/impulse-shell\" -c impulse-shell >\"${XDG_RUNTIME_DIR:-/tmp}/impulse-shell.log\" 2>&1 </dev/null"
        ),
        "the start script the files name is not there:\n{out}"
    );
    assert!(
        has_line(out, "settings: {\"bar\":{\"bottom\":true}}"),
        "the settings were not carried over:\n{out}"
    );
    assert!(
        has_line(out, "old settings: {\"bar\":{\"bottom\":true}}"),
        "{out}"
    );
}

#[test]
fn the_previous_shell_and_its_watch_stop_and_the_new_shell_runs() {
    let Some(out) = install("comes") else { return };
    assert!(has_line(&out, "install: status 0"), "{out}");
    assert!(has_line(&out, "new shell: runs"), "{out}");
    assert!(
        has_line(&out, "old shell: does not run"),
        "two shells run:\n{out}"
    );
    assert!(
        has_line(&out, "call: old watch: stop"),
        "the previous build's watch was left on:\n{out}"
    );
    assert!(
        !out.contains("call: old watch: enable"),
        "the previous build's watch was turned on again:\n{out}"
    );
    new_files(&out);
    assert!(
        has_line(
            &out,
            "keybinds.lua: hl.bind(\"CTRL + SUPER + R\", hl.dsp.exec_cmd(\"killall qs quickshell impulse-shell; $HOME/.local/bin/impulse-shell-start\"))"
        ),
        "{out}"
    );
    assert!(
        out.contains(&format!("install: back to {WAS}: ")),
        "the last lines do not say how to go back:\n{out}"
    );
}

#[test]
fn a_new_shell_that_does_not_come_up_leaves_the_previous_one_and_its_watch_running() {
    let Some(out) = install("fails") else { return };
    assert!(has_line(&out, "install: status 2"), "{out}");
    assert!(has_line(&out, "new shell: does not run"), "{out}");
    assert!(
        has_line(&out, "old shell: runs"),
        "the session is left without a shell:\n{out}"
    );
    assert!(
        has_line(
            &out,
            &format!("execs.lua: hl.exec_cmd(\"$HOME/.local/bin/{WAS}-shell\")")
        ),
        "the files were not put back:\n{out}"
    );
    let call = |what: &str| -> usize {
        let line = out
            .lines()
            .find_map(|l| l.strip_prefix(what))
            .unwrap_or_else(|| panic!("no line '{what}':\n{out}"));
        line.trim().parse().unwrap()
    };
    assert!(
        call("old shell started at call ") < call("old watch enabled at call "),
        "the watch was turned on before its shell was started:\n{out}"
    );
    assert!(
        has_line(&out, "old settings: {\"bar\":{\"bottom\":true}}"),
        "{out}"
    );
}

/// `--no-start`: the shell that runs keeps running with its watch; the watch is only kept from
/// starting at the next login, when the files start the new shell.
#[test]
fn without_a_start_the_previous_watch_is_only_kept_from_the_next_login() {
    let Some(out) = install("later") else { return };
    assert!(has_line(&out, "install: status 0"), "{out}");
    assert!(has_line(&out, "old shell: runs"), "{out}");
    assert!(has_line(&out, "new shell: does not run"), "{out}");
    assert!(
        has_line(
            &out,
            &format!("call: systemctl --user disable {WAS}-rollback-watch.service")
        ),
        "{out}"
    );
    assert!(
        !out.contains("call: old watch: stop"),
        "the watch of the shell that keeps running was stopped:\n{out}"
    );
    new_files(&out);
}
