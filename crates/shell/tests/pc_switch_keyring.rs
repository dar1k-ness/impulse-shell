//! `tools/pc/switch` moves the API keys: `tools/carry-over.sh` copies the keyring item of the
//! old build (application=illogical-impulse) to the new one's (application=rusty-impulse).
//! switch runs under `LC_ALL=C`, and there `secret-tool` printed a label or an attribute that
//! is not ASCII as question marks, or refused to store it ("Invalid byte sequence in conversion
//! input"): on a PC whose item has a Russian label the keys stayed behind, and the install said
//! "done" with status 0. Pinned here: such an item arrives whole (the secret byte for byte, the
//! label and the attributes as they were), and an item that cannot be written is an error the
//! install's last lines and its exit status tell.
//!
//! Each test has a home of its own with a real gnome-keyring on a session bus of its own;
//! `pc_switch_keyring/run.sh` lays it out and runs the repository's switch there.

use std::path::Path;
use std::process::Command;

/// The old item's label with the application's name changed, as carry-over.sh writes it.
const LABEL: &str = "Ключи rusty-impulse";
const EXPLANATION: &str = "Ключи для ИИ";

/// Runs switch in a new home (`case`: see run.sh). `None` when a tool is missing.
fn switch(case: &str) -> Option<String> {
    if !testenv::require_all(&[
        "unshare",
        "dbus-run-session",
        "gnome-keyring-daemon",
        "gdbus",
        "secret-tool",
        "jq",
        "flock",
        "pgrep",
    ]) {
        return None;
    }
    let home = tempfile::tempdir().unwrap();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = Command::new("bash")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pc_switch_keyring/run.sh"))
        .arg(&repo)
        .arg(case)
        .env("HOME", home.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if !stdout.contains("switch: status ") {
        testenv::unavailable(format!(
            "no PID namespace or no keyring (docker run --security-opt seccomp=unconfined --security-opt systempaths=unconfined): {stdout}{}",
            String::from_utf8_lossy(&out.stderr)
        ));
        return None;
    }
    Some(stdout)
}

fn has_line(out: &str, line: &str) -> bool {
    out.lines().any(|l| l == line)
}

#[test]
fn cyrillic_item_moves_with_its_keys() {
    let Some(out) = switch("moves") else { return };
    assert!(
        has_line(&out, "new item: secret equals the old one byte for byte"),
        "the keys are not in the new item:\n{out}"
    );
    assert!(
        has_line(&out, &format!("new item: label = {LABEL}")),
        "the new item's label is not the old one's:\n{out}"
    );
    assert!(
        has_line(
            &out,
            &format!("new item: attribute.explanation = {EXPLANATION}")
        ),
        "the new item's attribute is not the old one's:\n{out}"
    );
    assert!(has_line(&out, "old item: as it was"), "{out}");
    assert!(has_line(&out, "switch: status 0"), "{out}");
    assert!(
        out.contains("keyring: nothing is left to copy"),
        "switch does not say the keys moved:\n{out}"
    );
    assert!(has_line(&out, "shell: the new build runs"), "{out}");
}

#[test]
fn item_that_cannot_be_written_fails_the_install_loudly() {
    let Some(out) = switch("fails") else { return };
    assert!(has_line(&out, "new item: none"), "{out}");
    assert!(has_line(&out, "old item: as it was"), "{out}");
    assert!(
        has_line(&out, "switch: status 4"),
        "switch ended as if the keys had moved:\n{out}"
    );
    let last = out
        .lines()
        .filter(|l| l.starts_with("switch: "))
        .rev()
        .take(2)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        last.contains("KEYRING: FAILED. The new build runs WITHOUT the API keys")
            && last.contains("the keyring does not take this item"),
        "the last lines of switch do not say the keys did not move and why:\n{out}"
    );
    assert!(
        !out.contains("keyring: nothing is left to copy"),
        "switch says the keys moved:\n{out}"
    );
    // The install itself is not undone: the new build runs, without the keys.
    assert!(has_line(&out, "shell: the new build runs"), "{out}");
}
