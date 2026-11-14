//! The programs the theme code starts, each of which the original started from `switchwall.sh` (run by
//! a `Process`): every one gets the user's environment ([`workpool::userenv`]), not this
//! process's with the config's pragmas.
use crate::sys::Sys;
use std::path::{Path, PathBuf};

/// A program that appends what it sees of `RI_SITE_MARK` to `$RI_SITE_OUT/<its name>`.
const PROGRAM: &str = r#"#!/bin/sh
echo "${RI_SITE_MARK-unset}" >> "$RI_SITE_OUT/${0##*/}"
"#;

const NAMES: [&str; 4] = ["ri-run", "ri-spawn", "ri-reload", "xrdb"];

/// What the one run of `name` saw.
fn seen(out: &Path, name: &str) -> String {
    let f = out.join(name);
    let text = || std::fs::read_to_string(&f).unwrap_or_default();
    testenv::DirWatch::new(out).until(|| text().ends_with('\n'));
    text().trim().to_string()
}

#[test]
fn the_theme_s_programs_get_the_user_s_environment() {
    const TEST: &str = concat!(
        module_path!(),
        "::the_theme_s_programs_get_the_user_s_environment"
    );
    if !testenv::in_own_process(TEST) {
        let dir = tempfile::tempdir().unwrap();
        let (bin, out) = (dir.path().join("bin"), dir.path().join("out"));
        std::fs::create_dir(&bin).unwrap();
        std::fs::create_dir(&out).unwrap();
        for name in NAMES {
            testenv::install_script(&bin.join(name), PROGRAM);
        }
        let mut path = bin.into_os_string();
        path.push(":");
        path.push(std::env::var_os("PATH").unwrap());
        // This process's environment stands for the shell's own, pragmas applied.
        testenv::run_in_own_process(
            TEST,
            [
                ("PATH", path.as_os_str()),
                ("RI_SITE_OUT", out.as_os_str()),
                ("RI_SITE_MARK", "shell".as_ref()),
            ],
        );
        return;
    }
    let mut user: Vec<_> = std::env::vars_os()
        .filter(|(k, _)| k != "RI_SITE_MARK")
        .collect();
    user.push(("RI_SITE_MARK".into(), "user".into()));
    workpool::userenv::capture_as(user);
    let out = PathBuf::from(std::env::var_os("RI_SITE_OUT").unwrap());

    crate::live::Live.run(&["ri-run".into()], None);
    crate::live::Live.spawn(&["ri-spawn".into()]);
    crate::kde::pywal::spawn(&["ri-reload"]);
    crate::kde::run_hook("echo \"$RI_SITE_MARK\" >> \"$RI_SITE_OUT/hook\"");
    // A user's own console script, not the stock one: `sh` runs it.
    let cache = tempfile::tempdir().unwrap();
    std::fs::write(
        cache.path().join("colors-tty.sh"),
        "echo \"$RI_SITE_MARK\" >> \"$RI_SITE_OUT/console\"\n",
    )
    .unwrap();
    crate::kde::pywal::tty_console(&cache.path().join("colors-tty.sh"));
    crate::kde::pywal::xrdb_merge(&cache.path().join("colors.Xresources"));

    let sites = ["ri-run", "ri-spawn", "ri-reload", "hook", "xrdb", "console"];
    let got: Vec<String> = sites
        .iter()
        .map(|name| format!("{name}: {}", seen(&out, name)))
        .collect();
    let want: Vec<String> = sites.iter().map(|name| format!("{name}: user")).collect();
    assert_eq!(got, want, "what each program saw of RI_SITE_MARK");
}
