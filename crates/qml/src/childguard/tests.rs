use super::*;
use std::process::{Command, Stdio};

/// The test binary stands in for the shell binary: re-run with `GUARD_TEST=serve` it is the
/// guard, with `GUARD_TEST=shell` a shell that registers children and dies by SIGKILL.
fn me(test: &str, mode: &str) -> Command {
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.args([
        "--exact",
        &format!("childguard::tests::{test}"),
        "--nocapture",
    ])
    .args(["--test-threads=1"])
    .env("GUARD_TEST", mode);
    c
}

fn guard_command() -> Command {
    me("guard_main", "serve")
}

#[test]
fn guard_main() {
    if std::env::var("GUARD_TEST").is_ok_and(|m| m == "serve") {
        std::process::exit(serve());
    }
}

/// Running, i.e. neither gone nor a zombie (nothing may reap the orphans in a container).
fn running(pid: i32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| {
        s.rsplit_once(')')
            .is_some_and(|(_, r)| !r.trim_start().starts_with('Z'))
    })
}

use crate::testenv::{wait_all_gone, wait_gone};

/// Spawns `sh` leading its own group with a background member running `script`; (leader,
/// member). Neither exits on its own. With `ready`, also waits for the line `ready` that
/// `script` prints.
fn group_ready(script: &str, ready: Option<&str>) -> (std::process::Child, i32) {
    use std::io::BufRead;
    use std::os::unix::process::CommandExt;
    let mut ch = Command::new("sh")
        .args(["-c", &format!("{script} & echo $!; exec sleep infinity")])
        .stdout(Stdio::piped())
        .process_group(0)
        .spawn()
        .unwrap();
    let mut out = std::io::BufReader::new(ch.stdout.take().unwrap());
    let (mut member, mut seen) = (None, ready.is_none());
    while member.is_none() || !seen {
        let mut line = String::new();
        assert_ne!(
            out.read_line(&mut line).unwrap(),
            0,
            "the group closed its output"
        );
        match line.trim().parse() {
            Ok(pid) => member = Some(pid),
            Err(_) => seen |= Some(line.trim()) == ready,
        }
    }
    (ch, member.unwrap())
}

fn group(script: &str) -> (std::process::Child, i32) {
    group_ready(script, None)
}

#[test]
fn shell_killed_by_sigkill_takes_its_process_groups_along() {
    if std::env::var("GUARD_TEST").is_ok_and(|m| m == "shell") {
        enable(guard_command);
        // Ended by the guard: a plain group, and one whose member ignores SIGTERM.
        let (a, a_member) = group("sleep infinity");
        let (b, b_member) = group("trap '' TERM; sleep infinity");
        // Unregistered before the death: survives it, as a detached child does.
        let (c, c_member) = group("sleep infinity");
        for ch in [&a, &b, &c] {
            add(ch.id() as i32);
        }
        remove(c.id() as i32);
        assert!(pipe_fd().is_some());
        println!(
            "PIDS {} {a_member} {} {b_member} {} {c_member}",
            a.id(),
            b.id(),
            c.id()
        );
        use std::io::Write;
        std::io::stdout().flush().unwrap();
        // SAFETY: kill(2) takes plain integers; this re-run child kills itself on purpose.
        unsafe { libc::kill(std::process::id() as i32, libc::SIGKILL) };
        unreachable!();
    }
    let out = me(
        "shell_killed_by_sigkill_takes_its_process_groups_along",
        "shell",
    )
    .stderr(Stdio::inherit())
    .output()
    .unwrap();
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(out.status.signal(), Some(libc::SIGKILL));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let pids: Vec<i32> = stdout
        .lines()
        .find_map(|l| l.split_once("PIDS ").map(|(_, r)| r))
        .unwrap_or_else(|| panic!("{stdout}"))
        .split(' ')
        .map(|p| p.parse().unwrap())
        .collect();
    let [a, a_member, b, b_member, c, c_member] = pids[..] else {
        panic!("{pids:?}")
    };
    // None of them exits on its own: a registered group that outlived the shell, or a member
    // ignoring SIGTERM that was not killed after the grace period, would hang here.
    wait_all_gone(&[a, a_member, b, b_member]);
    assert!(
        running(c) && running(c_member),
        "an unregistered group was ended"
    );
    // SAFETY: kill(2) takes plain integers and touches no memory of ours.
    unsafe { libc::kill(-c, libc::SIGKILL) };
}

#[test]
fn nothing_starts_without_enable() {
    // `enable` is process-wide; this binary only enables it in re-run children.
    add(i32::MAX);
    assert!(pipe_fd().is_none());
    remove(i32::MAX);
}

/// A group that ends on SIGTERM ends `end_groups` as soon as its last member exits: the exits
/// are waited for as events, not the whole grace (here one that never ends).
#[test]
fn groups_ending_on_sigterm_are_not_waited_out() {
    let (mut leader, member) = group("sleep infinity");
    let g = leader.id() as i32;
    let (never, _open) = std::io::pipe().unwrap();
    end_groups(vec![g], Some(never.as_fd()));
    leader.wait().unwrap();
    wait_gone(member);
}

/// A member that ignores SIGTERM is SIGKILLed once the grace ends, and not before.
#[test]
fn a_group_ignoring_sigterm_is_killed_after_the_grace() {
    // The member ignores SIGTERM once it printed `ignoring` (SIG_IGN survives the exec).
    let (mut leader, member) = group_ready(
        "sh -c \"trap '' TERM; echo ignoring; exec sleep infinity\"",
        Some("ignoring"),
    );
    let g = leader.id() as i32;
    let (grace, over) = std::io::pipe().unwrap();
    let (done, ended) = std::sync::mpsc::channel();
    let h = std::thread::spawn(move || {
        end_groups(vec![g], Some(grace.as_fd()));
        done.send(()).unwrap();
    });
    // The SIGTERM ended the leader; the member ignores it and runs on while the grace lasts.
    crate::testenv::wait_exit(&mut leader);
    assert!(running(member), "killed before the grace ended");
    assert!(ended.try_recv().is_err(), "ended before the grace did");
    drop(over);
    h.join().unwrap();
    wait_gone(member);
}

/// A member forked after `end_groups` scanned the group (here by the leader's TERM trap) is
/// still waited for and SIGKILLed once the grace ends.
#[test]
fn a_member_forked_after_the_sigterm_is_killed_after_the_grace() {
    if std::env::var("GUARD_TEST").is_ok_and(|m| m == "late") {
        late_member_scenario();
        return;
    }
    let out = me(
        "a_member_forked_after_the_sigterm_is_killed_after_the_grace",
        "late",
    )
    .stderr(Stdio::inherit())
    .output()
    .unwrap();
    assert!(out.status.success(), "{out:?}");
}

/// Run as a re-exec'd child: as a subreaper it inherits the orphaned late member, so it can
/// wait for it and read how it ended.
fn late_member_scenario() {
    use std::io::BufRead;
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    // SAFETY: PR_SET_CHILD_SUBREAPER takes a plain integer flag.
    assert_eq!(unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1) }, 0);
    // Neither `sleep` ends unless something kills it.
    let mut leader = Command::new("sh")
        .args([
            "-c",
            "trap 'sleep infinity & echo $!; exit' TERM; sleep infinity & echo ready; wait",
        ])
        .stdout(Stdio::piped())
        .process_group(0)
        .spawn()
        .unwrap();
    let mut out = std::io::BufReader::new(leader.stdout.take().unwrap());
    let mut line = String::new();
    out.read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "ready");
    let (grace, over) = std::io::pipe().unwrap();
    let (done, ended) = std::sync::mpsc::channel();
    let g = leader.id() as i32;
    let h = std::thread::spawn(move || {
        end_groups(vec![g], Some(grace.as_fd()));
        done.send(()).unwrap();
    });
    leader.wait().unwrap();
    line.clear();
    out.read_line(&mut line).unwrap();
    let late: i32 = line.trim().parse().unwrap_or_else(|_| panic!("{line:?}"));
    // The leader is gone; the member it forked after the scan runs on while the grace lasts.
    assert!(running(late), "late member killed before the grace ended");
    assert!(ended.try_recv().is_err(), "ended before the grace did");
    drop(over);
    h.join().unwrap();
    let mut status = 0;
    // SAFETY: waitpid(2) writes the status of our (adopted) child `late` into `status`.
    assert_eq!(unsafe { libc::waitpid(late, &mut status, 0) }, late);
    let status = std::process::ExitStatus::from_raw(status);
    assert_eq!(
        status.signal(),
        Some(libc::SIGKILL),
        "late member: {status}"
    );
}
