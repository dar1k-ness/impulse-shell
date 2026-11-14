//! How `qml::easyeffects` runs the long-lived `a` of `a || b` (ENABLE's
//! `easyeffects --service-mode`, OPEN's `flatpak run`): no copy of the shell process may
//! outlive the call (it would hold every shell fd, CLOEXEC ones included, and its memory for
//! as long as the service runs), no worker of the shared pool may be parked waiting on it,
//! `b` still runs when `a` fails (not when it is stopped), and `a` is reaped.
//! Not named easyeffects*: pkill matches the 15-byte comm of the test binary.
use qml::easyeffects as ee;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static LOCK: Mutex<()> = Mutex::new(());

/// `easyeffects -q` asks the running service to quit, as EasyEffects does over its local
/// socket: here the stub ends the services it started.
const A: &str = r#"#!/bin/bash
if [ "$1" = -q ]; then
    echo quit >> "$EE_DIR/quits"
    while read -r p; do kill "$p" 2>/dev/null; done < "$EE_DIR/a.pids"
    exit 0
fi
ls -l /proc/$$/fd > "$EE_DIR/a.fds.$$" 2>&1
[ -n "$EE_A_EXIT" ] && { echo $$ >> "$EE_DIR/a.pids"; exit "$EE_A_EXIT"; }
# Listed only once `pidof easyeffects` (which `disable` asks) sees it, and seen for as long as
# it runs: a script's argv[0] is its interpreter, so the stub is not an `easyeffects` before
# this exec, and it execs no more after it (during an exec /proc/PID/cmdline is empty and pidof
# passes the process by): `flock` is its child, and the `exit` after it keeps bash from
# exec'ing the last command of `-c`.
# It runs for as long as the test holds its lock on `alive` (see `Env::stubs`), so it ends with
# the test process however that one ends; its status says whether `alive` held anything by then.
exec -a easyeffects bash -c 'echo $$ >> "$EE_DIR/a.pids"; flock -s "$EE_DIR/alive" test -s "$EE_DIR/alive"; exit'
"#;

/// `flatpak run APP` alone is OPEN's `a`: it becomes the same long-running stub.
const B: &str = r#"#!/bin/bash
[ "$#" = 2 ] && [ "$1" = run ] && exec "$EE_DIR/bin/easyeffects"
echo "$*" >> "$EE_DIR/b.runs"
"#;

struct Env {
    dir: PathBuf,
}

impl Env {
    /// Runs `test` for `step` in its own process (see `testenv::run_in_own_process`) with
    /// PATH holding the stub `easyeffects` and `flatpak` of a fresh directory; `a_exit` None
    /// keeps `a` running.
    fn run(test: &str, step: &str, a_exit: Option<i32>) {
        let (dir, _alive) = Env::stubs();
        let bin = dir.path().join("bin");
        let path = format!("{}:/usr/bin:/bin", bin.display());
        // Empty is the stub's "keep running", as unset is.
        let a_exit = a_exit.map(|c| c.to_string()).unwrap_or_default();
        testenv::run_in_own_process(
            test,
            [
                ("PATH", OsString::from(path)),
                ("EE_DIR", dir.path().into()),
                ("EE_A_EXIT", a_exit.into()),
                ("EE_STEP", step.into()),
            ],
        );
    }

    /// A fresh directory with the stub `easyeffects` and `flatpak` in its `bin`, and the lock
    /// that keeps a long-running stub alive: the kernel releases it when the returned file is
    /// closed, which a killed process's is too (nextest ending a hung test runs no `Drop`), and
    /// a stub left behind would fail every later test that needs no `easyeffects` running.
    fn stubs() -> (tempfile::TempDir, std::fs::File) {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        for (n, s) in [("easyeffects", A), ("flatpak", B)] {
            testenv::install_script(&bin.join(n), s);
        }
        let alive = std::fs::File::create(dir.path().join("alive")).unwrap();
        alive.lock().unwrap();
        (dir, alive)
    }

    /// In the process `run` started: its stubs and step.
    fn here() -> (Env, String) {
        let dir = PathBuf::from(std::env::var_os("EE_DIR").unwrap());
        (Env { dir }, std::env::var("EE_STEP").unwrap())
    }

    fn read(&self, f: &str) -> String {
        std::fs::read_to_string(self.dir.join(f)).unwrap_or_default()
    }

    fn a_pids(&self) -> Vec<i32> {
        self.read("a.pids")
            .lines()
            .filter_map(|l| l.parse().ok())
            .collect()
    }

    fn b_runs(&self) -> Vec<String> {
        self.read("b.runs").lines().map(String::from).collect()
    }

    /// Blocks until `ok` holds, checking it again on every change to the stubs' files
    /// (inotify). `a` writes its fd listing before its pid, so a pid listed has its listing.
    fn wait(&self, what: &str, ok: impl Fn(&Env) -> bool) {
        // Named in the output nextest shows when its slow-timeout ends a hang.
        eprintln!("waiting for {what}");
        testenv::DirWatch::new(&self.dir).until(|| ok(self));
    }

    /// Stops every `a` with SIGTERM, as a logout does, and waits until the pool reaped them
    /// and ran whatever their end started: a stopped service is no failure to fall back from.
    fn stop_all(&self) {
        let pids = self.a_pids();
        for &p in &pids {
            // SAFETY: kill(2) takes no pointers, so it has no memory-safety precondition.
            unsafe { libc::kill(p, libc::SIGTERM) };
        }
        for p in pids {
            testenv::wait_gone(p);
        }
        workpool::wait_idle();
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        for p in self.a_pids() {
            // SAFETY: kill(2) takes no pointers, so it has no memory-safety precondition.
            unsafe { libc::kill(p, libc::SIGKILL) };
        }
    }
}

fn pids() -> Vec<i32> {
    std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse().ok())
        .collect()
}

/// This process and the ones it descends from (the test process that started it here).
fn lineage() -> Vec<i32> {
    let mut v = vec![std::process::id() as i32];
    while let Some(pp) = parent(*v.last().unwrap()).filter(|&pp| pp > 0) {
        v.push(pp);
    }
    v
}

fn parent(pid: i32) -> Option<i32> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    s[s.rfind(')')? + 2..].split(' ').nth(1)?.parse().ok()
}

/// Processes outside this one's lineage still running this test binary's image (fork
/// copies).
fn copies_of_self() -> Vec<i32> {
    let me = std::fs::read_link("/proc/self/exe").unwrap();
    let lineage = lineage();
    pids()
        .into_iter()
        .filter(|p| !lineage.contains(p))
        .filter(|p| std::fs::read_link(format!("/proc/{p}/exe")).is_ok_and(|e| e == me))
        .filter(|p| state(*p).is_some_and(|s| s != 'Z'))
        .collect()
}

/// Processes other than this one with an fd on `target` (e.g. `socket:[123]`).
fn holders(target: &Path) -> Vec<i32> {
    let mine = std::process::id() as i32;
    pids()
        .into_iter()
        .filter(|&p| p != mine)
        .filter(|p| {
            std::fs::read_dir(format!("/proc/{p}/fd"))
                .into_iter()
                .flatten()
                .flatten()
                .any(|e| std::fs::read_link(e.path()).is_ok_and(|l| l == target))
        })
        .collect()
}

fn state(pid: i32) -> Option<char> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    s[s.rfind(')')? + 2..].chars().next()
}

/// The stub service ends when the test process lets go of its lock, which is what happens
/// when that process is killed, and not before: it sees what was written to `alive` under the
/// lock, after it started.
#[test]
fn stub_service_ends_with_the_lock_of_its_test() {
    use std::io::Write;
    let (dir, mut alive) = Env::stubs();
    let e = Env {
        dir: dir.path().into(),
    };
    let mut a = std::process::Command::new(dir.path().join("bin/easyeffects"))
        .env("EE_DIR", dir.path())
        .env_remove("EE_A_EXIT")
        .spawn()
        .unwrap();
    e.wait("a to start", |e| !e.a_pids().is_empty());
    alive.write_all(b"released").unwrap();
    drop(alive);
    assert!(
        testenv::wait_exit(&mut a).success(),
        "the stub did not wait for the lock: `alive` was empty when it took it"
    );
}

/// ENABLE with a service that keeps running: nothing but the service itself lives on, it
/// does not hold this process's (CLOEXEC) fds, and its stdout/stderr are /dev/null. `b`
/// never ran meanwhile (checked once every `a` was stopped, which starts no `b` either).
#[test]
fn enable_leaves_no_shell_copy_holding_fds() {
    let test = concat!(module_path!(), "::enable_leaves_no_shell_copy_holding_fds");
    if !testenv::in_own_process(test) {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        return Env::run(test, "fds", None);
    }
    let (sock, _peer) = std::os::unix::net::UnixStream::pair().unwrap();
    use std::os::fd::AsRawFd;
    let target = std::fs::read_link(format!("/proc/self/fd/{}", sock.as_raw_fd())).unwrap();
    let (e, _) = Env::here();
    for (n, (what, f)) in [
        ("enable", (|| ee::enable(None)) as fn()),
        ("open", ee::open),
    ]
    .into_iter()
    .enumerate()
    {
        f();
        // `a` runs (exec'd, so no fork copy of this process stands in front of it) and has
        // listed its fds: a copy or holder of ours would be alive now, as long as `a` runs.
        e.wait("a to start", |e| e.a_pids().len() > n);
        assert_eq!(
            copies_of_self(),
            Vec::<i32>::new(),
            "{what}: fork copy of the shell alive"
        );
        assert_eq!(
            holders(&target),
            Vec::<i32>::new(),
            "{what}: shell fd held by another process"
        );
        for p in e.a_pids() {
            let fds = e.read(&format!("a.fds.{p}"));
            for fd in ["1", "2"] {
                assert!(
                    fds.lines()
                        .any(|l| l.contains(&format!(" {fd} -> /dev/null"))),
                    "{what}: fd {fd} of a not /dev/null:\n{fds}"
                );
            }
            assert!(
                !fds.contains(&target.display().to_string()),
                "{what}: {fds}"
            );
        }
    }
    e.stop_all();
    assert!(
        e.b_runs().is_empty(),
        "b ran while a was running: {:?}",
        e.b_runs()
    );
}

/// An fd the shell holds without CLOEXEC (inherited from its launcher, or opened by a C
/// library) must not leak into the service either: it would keep that pipe/socket open for
/// as long as EasyEffects runs.
#[test]
fn service_inherits_no_non_cloexec_fd() {
    let test = concat!(module_path!(), "::service_inherits_no_non_cloexec_fd");
    if !testenv::in_own_process(test) {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        return Env::run(test, "nocloexec", None);
    }
    let mut p = [0i32; 2];
    // SAFETY: plain pipe(2) without O_CLOEXEC.
    assert_eq!(unsafe { libc::pipe(p.as_mut_ptr()) }, 0);
    let target = std::fs::read_link(format!("/proc/self/fd/{}", p[0])).unwrap();
    let (e, _) = Env::here();
    ee::enable(None);
    e.wait("a to start", |e| !e.a_pids().is_empty());
    let fds: String = e
        .a_pids()
        .iter()
        .map(|p| e.read(&format!("a.fds.{p}")))
        .collect();
    // SAFETY: `p[0]` is the read end pipe(2) opened above; nothing else owns or closes it.
    unsafe { libc::close(p[0]) };
    // SAFETY: `p[1]` is the write end pipe(2) opened above; nothing else owns or closes it.
    unsafe { libc::close(p[1]) };
    assert!(
        !fds.contains(&target.display().to_string()),
        "service inherited the shell's non-CLOEXEC {}:\n{fds}",
        target.display()
    );
}

/// Long-running `a`s must not each park a worker of the shared pool (`workpool::WORKERS`):
/// files.rs, hyprctl and D-Bus jobs queue on it.
#[test]
fn running_services_do_not_park_pool_workers() {
    let test = concat!(
        module_path!(),
        "::running_services_do_not_park_pool_workers"
    );
    if !testenv::in_own_process(test) {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        return Env::run(test, "pool", None);
    }
    let (e, _) = Env::here();
    for _ in 0..workpool::WORKERS {
        ee::enable(None);
    }
    ee::open();
    e.wait("every a to start", |e| e.a_pids().len() > workpool::WORKERS);
    let (tx, rx) = std::sync::mpsc::channel();
    workpool::run(move || tx.send(()).unwrap());
    // A hang here (ended by nextest's slow-timeout) is the starvation: every worker waits on
    // a running service.
    eprintln!("waiting for a pool job behind the running services");
    rx.recv().expect("pool job ran");
}

/// `a || b`: b runs once when a exits non-zero, never when a exits 0 or is killed by a
/// signal (unlike bash's 128+sig); a is reaped either way (no zombie left behind).
#[test]
fn b_runs_exactly_when_a_fails_and_a_is_reaped() {
    let test = concat!(
        module_path!(),
        "::b_runs_exactly_when_a_fails_and_a_is_reaped"
    );
    if !testenv::in_own_process(test) {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        for code in [0, 1, 127] {
            Env::run(test, "exit", Some(code));
        }
        Env::run(test, "killed", None);
        Env::run(test, "disable", None);
        return;
    }
    let (e, step) = Env::here();
    match step.as_str() {
        "exit" => {
            let code: i32 = std::env::var("EE_A_EXIT").unwrap().parse().unwrap();
            let b = code != 0;
            ee::enable(None);
            e.wait("a", |e| !e.a_pids().is_empty());
            // `a` ended by itself; the pool reaped it and ran `b` if it failed.
            workpool::wait_idle();
            let want: Vec<String> = if b {
                vec![format!("run {} --hide-window --service-mode", ee::APP)]
            } else {
                vec![]
            };
            assert_eq!(e.b_runs(), want, "a exit {code}");
            for p in e.a_pids() {
                assert_ne!(state(p), Some('Z'), "a (exit {code}) left a zombie");
            }
        }
        "killed" => {
            ee::enable(None);
            e.wait("a", |e| !e.a_pids().is_empty());
            let a = e.a_pids()[0];
            e.stop_all();
            assert_eq!(state(a), None, "a reaped after SIGTERM");
            // The original (bash) starts the flatpak copy here; a stopped service is no failure to fall
            // back from.
            assert_eq!(e.b_runs(), Vec::<String>::new(), "b after a was killed");
        }
        "disable" => {
            // `disable` on a running service: the original's bash then runs b of ENABLE, so turning
            // EasyEffects off starts the flatpak one. Not here.
            ee::enable(None);
            e.wait("a", |e| !e.a_pids().is_empty());
            let a = e.a_pids()[0];
            // What `disable` asks: an `a` it did not see would never be told to quit.
            assert!(
                qml::osutil::pidof("easyeffects").contains(&a),
                "a ({a}) listed its pid, yet `pidof easyeffects` does not see it"
            );
            ee::disable(None);
            // The quit ran on the pool and stopped `a`, which the pool reaped.
            workpool::wait_idle();
            assert_eq!(state(a), None, "a stopped by disable");
            assert_eq!(
                e.read("quits"),
                "quit\n",
                "disable quits through `easyeffects -q`"
            );
            assert_eq!(e.b_runs(), Vec::<String>::new(), "b after disable");
        }
        s => panic!("no step {s}"),
    }
}
