//! The environment of the shell and of the programs it starts, the real Quickshell vs the
//! native shell on the same config. Quickshell copies the environment it was started with
//! before it applies the pragmas (`Common::INITIAL_ENVIRONMENT`, src/launch/launch.cpp) and
//! hands that copy to `Process`, `Process.startDetached()` and `Quickshell.execDetached`
//! (`setupProcessEnvironment`, src/io/processcore.cpp): `//@ pragma Env`,
//! `//@ pragma DefaultEnv`, the Fusion style and the removal of `QT_STYLE_OVERRIDE` reach the
//! shell alone.
use std::path::Path;
use std::process::{Child, Command, Stdio};

use testenv::Log;

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

const NATIVE: &str = env!("CARGO_BIN_EXE_impulse-shell");

/// The variables asked about, in the order they are reported.
const VARS: [&str; 5] = [
    "QT_QUICK_CONTROLS_STYLE",
    "FOO",
    "QT_SCALE_FACTOR",
    "BAR",
    "QT_STYLE_OVERRIDE",
];

/// What the user's session has set, for the case with all of them.
const USER: [(&str, &str); 5] = [
    ("QT_QUICK_CONTROLS_STYLE", "Material"),
    ("FOO", "user"),
    ("QT_SCALE_FACTOR", "2"),
    ("BAR", "user"),
    ("QT_STYLE_OVERRIDE", "kvantum"),
];

const PRAGMAS: &str = "//@ pragma Env QS_NO_RELOAD_POPUP=1
//@ pragma Env QT_QUICK_CONTROLS_STYLE=Basic
//@ pragma Env FOO=1
//@ pragma Env QT_SCALE_FACTOR=1
//@ pragma DefaultEnv BAR=1
";

/// Pragma lines around what is neither a comment line nor an import. Quickshell reads the
/// lines that start with `//@ pragma ` up to the first line that starts with `import`, and
/// parses nothing else.
const AROUND_COMMENTS: &str = "/*
 * A header.
//@ pragma Env QT_STYLE_OVERRIDE=inside
 */
//@ pragma Env QS_NO_RELOAD_POPUP=1
//@ pragma Env FOO=after
// A note.

   //@ pragma Env BAR=indented
/* one line */ //@ pragma Env QT_SCALE_FACTOR=3
//@pragma Env QT_QUICK_CONTROLS_STYLE=Basic
";

/// The shell says what it sees itself (`SELF`), and four programs write what they see to
/// `$OUT/<name>`: a `Process`, a `Process` with `environment`, `Process.startDetached()` and
/// `Quickshell.execDetached`. `t.reload` over IPC reloads the shell.
fn fixture(pragmas: &str) -> String {
    let dump: String = VARS
        .iter()
        .map(|v| format!("{v}=[${{{v}-unset}}] "))
        .collect();
    let own: String = VARS
        .iter()
        .map(|v| format!("\"{v}=[\" + seen(\"{v}\") + \"] \""))
        .collect::<Vec<_>>()
        .join(" + ");
    format!(
        r#"{pragmas}import QtQuick
import Quickshell
import Quickshell.Io
ShellRoot {{
    id: root
    function dump(name) {{
        return "echo \"{dump}\" >\"$OUT/" + name + ".tmp\" && mv \"$OUT/" + name + ".tmp\" \"$OUT/" + name + "\""
    }}
    // A variable that is not set reads `null` in Quickshell.
    function seen(name) {{
        const v = Quickshell.env(name)
        return v == null ? "unset" : v
    }}
    IpcHandler {{
        target: "t"
        function reload(): void {{
            Quickshell.reload(true)
        }}
    }}
    Process {{
        running: true
        command: ["sh", "-c", root.dump("process")]
    }}
    Process {{
        running: true
        environment: ({{ EXTRA: "x" }})
        command: ["sh", "-c", root.dump("process_environment")]
    }}
    Process {{
        id: detached
        command: ["sh", "-c", root.dump("start_detached")]
    }}
    Component.onCompleted: {{
        detached.startDetached()
        Quickshell.execDetached(["sh", "-c", root.dump("exec_detached")])
        console.log("SELF " + {own})
    }}
}}
"#
    )
}

/// What `bin` serving `dir` sees itself and what each of its four programs sees, with `user`
/// set in the session and the other [`VARS`] not set. With `reload`, what they see once the
/// shell was reloaded.
fn seen(bin: &str, dir: &Path, user: &[(&str, &str)], reload: bool) -> Vec<(String, String)> {
    let out = dir.join("out");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).unwrap();
    let (rd, wr) = std::io::pipe().unwrap();
    let mut c = Command::new(bin);
    c.arg("-p").arg(dir);
    for v in VARS {
        c.env_remove(v);
    }
    let child = c
        .env("RI_INTERP", "1")
        .env("OUT", &out)
        .envs(user.iter().copied())
        .stdin(Stdio::null())
        .stdout(wr.try_clone().unwrap())
        .stderr(wr)
        .spawn()
        .unwrap();
    drop(c);
    let _k = Kill(child);
    let log = Log::capture(rd);
    // What the `load`th load of the fixture showed.
    let shown = |load: usize| {
        log.wait(|l| l.matches("SELF ").count() >= load)
            .unwrap_or_else(|l| panic!("{bin} never loaded the fixture ({load}):\n{l}"));
        let text = log.text();
        let own = text
            .lines()
            .filter_map(|l| l.split_once("SELF ").map(|(_, v)| v.trim().to_string()))
            .nth(load - 1)
            .unwrap();
        let mut all = vec![("the shell".to_string(), own)];
        for name in [
            "process",
            "process_environment",
            "start_detached",
            "exec_detached",
        ] {
            let f = out.join(name);
            // Renamed into place once written.
            testenv::wait_exists(&f);
            let text = std::fs::read_to_string(&f).unwrap();
            all.push((name.to_string(), text.trim().to_string()));
            std::fs::remove_file(&f).unwrap();
        }
        all
    };
    let first = shown(1);
    if !reload {
        return first;
    }
    let call = Command::new(bin)
        .args(["ipc", "-p"])
        .arg(dir)
        .args(["call", "t", "reload"])
        .env("RI_INTERP", "1")
        .output()
        .unwrap();
    assert!(call.status.success(), "{bin}: {call:?}");
    shown(2)
}

/// Both shells, with the same session: every reader sees the same in both.
#[track_caller]
fn same(pragmas: &str, user: &[(&str, &str)]) {
    same_after(pragmas, user, false);
}

/// [`same`]; with `reload`, once each shell was reloaded.
#[track_caller]
fn same_after(pragmas: &str, user: &[(&str, &str)], reload: bool) {
    let base = tempfile::tempdir().unwrap();
    let mut both = Vec::new();
    for (bin, name) in [("qs", "orig"), (NATIVE, "native")] {
        let d = base.path().join(name);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("shell.qml"), fixture(pragmas)).unwrap();
        both.push(seen(bin, &d, user, reload));
    }
    let (want, got) = (&both[0], &both[1]);
    let differing: Vec<String> = want
        .iter()
        .zip(got)
        .filter(|(w, g)| w != g)
        .map(|((who, w), (_, g))| format!("{who}:\n  quickshell {w}\n  native     {g}"))
        .collect();
    assert!(
        differing.is_empty(),
        "with {user:?} in the session, reload {reload} and the pragmas\n{pragmas}{}",
        differing.join("\n")
    );
}

fn ready() -> bool {
    if !testenv::require("qs") {
        return false;
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return false;
    }
    true
}

/// `pragma Env` and `pragma DefaultEnv` with nothing set in the session: the shell sees the
/// pragmas' values, its programs see none of the variables.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn pragmas_reach_the_shell_alone_when_the_session_sets_nothing() {
    if ready() {
        same(PRAGMAS, &[]);
    }
}

/// The same with every variable set in the session: `Env` wins in the shell, `DefaultEnv`
/// does not, `QT_STYLE_OVERRIDE` is gone from the shell, and its programs see the session's
/// values.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn programs_get_the_session_environment_not_the_pragmas() {
    if ready() {
        same(PRAGMAS, &USER);
    }
}

/// Without pragmas the shell still runs with the Fusion style and without
/// `QT_STYLE_OVERRIDE`; its programs see the session's values.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn the_style_variables_are_the_shell_s_own() {
    if ready() {
        same("", &[]);
        same("", &USER);
    }
}

/// `pragma RespectSystemStyle` keeps the session's style variables in the shell.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn respect_system_style_keeps_the_session_s_style() {
    if ready() {
        same("//@ pragma RespectSystemStyle\n", &USER);
    }
}

/// A reloaded shell and the programs it starts see what they saw before the reload: the
/// session's environment stays the one the shell was first started with.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_reload_keeps_the_session_environment() {
    if ready() {
        same_after(PRAGMAS, &USER, true);
        same_after(PRAGMAS, &[], true);
    }
}

/// A block comment does not end the reading of the pragmas: the lines after it and inside
/// it are read, a line with something before `//@` and one without the space are not.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn pragmas_are_read_past_a_block_comment() {
    if !ready() {
        return;
    }
    same(AROUND_COMMENTS, &[]);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("shell.qml"), fixture(AROUND_COMMENTS)).unwrap();
    let own = seen(NATIVE, dir.path(), &[], false).swap_remove(0).1;
    assert_eq!(
        own,
        "QT_QUICK_CONTROLS_STYLE=[Fusion] FOO=[after] QT_SCALE_FACTOR=[unset] BAR=[indented] \
         QT_STYLE_OVERRIDE=[inside]"
    );
}

/// A pragma after the first `import` is not read, in a comment or not.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn pragmas_after_the_first_import_are_not_read() {
    if !ready() {
        return;
    }
    // The fixture's imports follow the pragmas, so this one opens them.
    let pragmas = "//@ pragma Env QS_NO_RELOAD_POPUP=1\n/* a note */\nimport QtQml\n\
                   //@ pragma Env FOO=late\n/*\n//@ pragma Env BAR=late\n*/\n";
    same(pragmas, &[]);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("shell.qml"), fixture(pragmas)).unwrap();
    let own = seen(NATIVE, dir.path(), &[], false).swap_remove(0).1;
    assert!(
        own.contains("FOO=[unset] ") && own.contains("BAR=[unset] "),
        "{own}"
    );
}

/// `Quickshell.env` of a variable that is not set is `null`, not `undefined`; of one set to
/// nothing, the empty string.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn quickshell_env_of_an_unset_variable_is_null() {
    if !ready() {
        return;
    }
    let base = tempfile::tempdir().unwrap();
    let said = |bin: &str, name: &str| -> String {
        let dir = base.path().join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("shell.qml"),
            r#"//@ pragma Env QS_NO_RELOAD_POPUP=1
import QtQuick
import Quickshell
ShellRoot {
    function kind(v) {
        return v === null ? "null" : v === undefined ? "undefined" : typeof v + " [" + v + "]"
    }
    Component.onCompleted: console.log("ENV unset " + kind(Quickshell.env("PROBE_UNSET"))
        + ", empty " + kind(Quickshell.env("PROBE_EMPTY"))
        + ", set " + kind(Quickshell.env("PROBE_SET"))
        + ", no name " + kind(Quickshell.env("")) + " END")
}
"#,
        )
        .unwrap();
        let (rd, wr) = std::io::pipe().unwrap();
        let mut c = Command::new(bin);
        let child = c
            .arg("-p")
            .arg(&dir)
            .env("RI_INTERP", "1")
            .env_remove("PROBE_UNSET")
            .env("PROBE_EMPTY", "")
            .env("PROBE_SET", "a b")
            .stdin(Stdio::null())
            .stdout(wr.try_clone().unwrap())
            .stderr(wr)
            .spawn()
            .unwrap();
        drop(c);
        let _k = Kill(child);
        let log = Log::capture(rd);
        log.wait(|l| l.contains(" END"))
            .unwrap_or_else(|l| panic!("{bin} never loaded the fixture:\n{l}"));
        let text = log.text();
        let from = text.find("ENV unset ").unwrap();
        text[from..text.find(" END").unwrap()].to_string()
    };
    let want = said("qs", "orig");
    assert_eq!(
        want,
        "ENV unset null, empty string [], set string [a b], no name null"
    );
    assert_eq!(said(NATIVE, "native"), want);
}
