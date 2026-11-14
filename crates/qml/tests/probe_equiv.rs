//! The original's process/directory probes against the original commands (`qs -c rusty-impulse` runs them through
//! bash): the same fake processes and directories go through `bash -c "<the original's line>"` and
//! `qml::probe`, and stdout, stderr and exit code must match. Run with Arch's procps-ng /
//! psmisc / coreutils / util-linux (Ubuntu's `pidof` is sysvinit's).

use qml::probe::{self, Out};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use testenv::Running;

fn bash_env(script: &str, env: &[(&str, &str)]) -> Out {
    let o = Command::new("bash")
        .arg("-c")
        .arg(script)
        .envs(env.iter().copied())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    Out {
        out: String::from_utf8_lossy(&o.stdout).into_owned(),
        err: String::from_utf8_lossy(&o.stderr).into_owned(),
        code: o.status.code().unwrap_or(-1),
    }
}

fn bash(script: &str) -> Out {
    bash_env(script, &[])
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ri-probe-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A live process named `name` (a renamed `cat`, copied out of process so no fd of ours on it
/// makes running it fail with ETXTBSY), running when this returns; killed on drop.
struct Fake(Running);

impl Fake {
    fn new(dir: &Path, name: &str) -> Fake {
        let p = testenv::cat_copy(&dir.join(name));
        Fake(Running::spawn(&mut Command::new(p)))
    }
}

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

// Fake names are unique to each test (tests run in parallel and pidof sees them all).

#[test]
fn running_names_like_pidof() {
    let d = tmp("running");
    let _a = Fake::new(&d, "ri-probe-run-a");
    let _b = Fake::new(&d, "ri-probe-run-b");
    let all = names(&["ri-probe-run-a", "ri-probe-run-none", "ri-probe-run-b"]);
    // `pidof NAME` exits 0 exactly for the names `running` gives.
    for n in &all {
        let found = bash(&format!("pidof {n}")).code == 0;
        assert_eq!(qml::processes::running(&all).contains(n), found, "{n}");
    }
    assert_eq!(
        qml::processes::running(&all),
        names(&["ri-probe-run-a", "ri-probe-run-b"])
    );
    assert!(qml::processes::running(&names(&["ri-probe-run-none"])).is_empty());
}

#[test]
fn pidof_like_procps() {
    let d = tmp("pidof");
    let n = names(&["ri-probe-rec"]);
    assert_eq!(probe::pidof(&n), bash("pidof ri-probe-rec"));
    let _a = Fake::new(&d, "ri-probe-rec");
    let _b = Fake::new(&d, "ri-probe-rec2");
    let _c = Fake::new(&d, "ri-probe-rec");
    assert_eq!(probe::pidof(&n), bash("pidof ri-probe-rec"));
    let two = names(&["ri-probe-rec2", "ri-probe-none", "ri-probe-rec"]);
    assert_eq!(
        probe::pidof(&two),
        bash("pidof ri-probe-rec2 ri-probe-none ri-probe-rec")
    );
}

#[test]
fn killall_like_psmisc() {
    let d = tmp("killall");
    let run = |native: bool| {
        let mut a = Fake::new(&d, "ri-probe-ka");
        let mut b = Fake::new(&d, "ri-probe-ka");
        let n = names(&["ri-probe-ka", "ri-probe-kb"]);
        let o = if native {
            probe::killall(&n)
        } else {
            bash("killall ri-probe-ka ri-probe-kb")
        };
        // killall has returned: a target it signalled is ending, one it missed still answers.
        let dead = [
            a.0.alive().map_err(|s| s.code()),
            b.0.alive().map_err(|s| s.code()),
        ];
        (o, dead)
    };
    assert_eq!(run(true), run(false));
    let n = names(&["ri-probe-kc"]);
    assert_eq!(probe::killall(&n), bash("killall ri-probe-kc"));
}

#[test]
fn pidof_or_start_detached() {
    let d = tmp("ensure");
    let prog = d.join("ri-probe-daemon");
    // The started program forks the long-running process and exits once it runs, so the next
    // `pidof` sees it as soon as the start is done (`testenv::install_daemon`).
    testenv::install_daemon(&prog, "");
    let p = prog.to_str().unwrap().to_string();
    let pids = || -> Vec<i32> {
        probe::pidof(std::slice::from_ref(&p))
            .out
            .split_whitespace()
            .map(|s| s.parse().unwrap())
            .collect()
    };
    let kill = || {
        let before = pids();
        let _ = probe::killall(&names(&["ri-probe-daemon"]));
        for pid in before {
            testenv::wait_gone(pid);
        }
    };
    // `execDetached(["bash", "-c", "pidof P || P"])`, twice, the second once the first is done.
    for native in [true, false] {
        for _ in 0..2 {
            if native {
                qml::processes::start_unless_running(&p);
                // The start runs on the pool, which reaps the starter once P runs.
                workpool::wait_idle();
            } else {
                let mut sh = Command::new("bash")
                    .args(["-c", &format!("pidof {p} || {p}")])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .spawn()
                    .unwrap();
                testenv::wait_exit(&mut sh);
            }
        }
        assert_eq!(pids().len(), 1, "native {native}");
        kill();
        assert_eq!(pids().len(), 0);
    }
}
