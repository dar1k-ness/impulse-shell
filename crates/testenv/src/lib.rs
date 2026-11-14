//! What a test does when the original it compares against (a reference tool, a service, a
//! compositor) is missing: it fails, so a green run means every comparison really ran.
//! Exactly `RI_EQUIV_SKIP=1` turns that into a printed skip, for a machine knowingly without
//! them; any other value (`0`, `yes`, empty) still fails. tools/testenv/Containerfile is an
//! environment that has them all.

// std's eprintln! panics once stderr's reader is gone; `elog`'s drops the line.
#![warn(clippy::print_stderr)]

/// Fails the calling test because `what` is missing for its comparison with the original; with
/// `RI_EQUIV_SKIP=1` only prints it and returns, and the caller skips the comparison.
#[track_caller]
pub fn equiv_missing(what: impl std::fmt::Display) {
    if skip_allowed() {
        elog::eprintln!("SKIPPED (RI_EQUIV_SKIP=1): needs {what}");
        return;
    }
    panic!(
        "equivalence reference missing: {what}. Install the reference tools \
         (tools/testenv/Containerfile has them all) or set RI_EQUIV_SKIP=1 to skip knowingly"
    );
}

/// [`equiv_missing`] under the name the tests already use.
#[track_caller]
pub fn unavailable(why: impl std::fmt::Display) {
    equiv_missing(why)
}

/// `equiv_missing("`tool` is not installed")` unless `tool` is on PATH.
#[track_caller]
pub fn require(tool: &str) -> bool {
    let found = on_path(tool);
    if !found {
        equiv_missing(format_args!("`{tool}` is not installed"));
    }
    found
}

/// Every tool is on PATH; reports each missing one (all of them, not just the first).
#[track_caller]
pub fn require_all(tools: &[&str]) -> bool {
    let missing: Vec<_> = tools
        .iter()
        .filter(|t| !on_path(t))
        .map(|t| format!("`{t}`"))
        .collect();
    if !missing.is_empty() {
        equiv_missing(format_args!("{} not installed", missing.join(", ")));
    }
    missing.is_empty()
}

/// Some PATH entry holds an executable `name`, as bash/which search it.
pub fn on_path(name: &str) -> bool {
    path_lookup(name, &std::env::var("PATH").unwrap_or_default())
}

/// Executable regular file (what execvp / bash's PATH search accept).
fn is_exec(p: &std::path::Path) -> bool {
    let Ok(c) = std::ffi::CString::new(p.as_os_str().as_encoded_bytes()) else {
        return false;
    };
    // SAFETY: access(2) only reads the NUL-terminated path `c`, alive for the call.
    p.is_file() && unsafe { libc::access(c.as_ptr(), libc::X_OK) } == 0
}

/// Some PATH entry holds an executable `name`, as bash/which search it.
fn path_lookup(name: &str, path: &str) -> bool {
    path.split(':').any(|d| {
        let d = if d.is_empty() { "." } else { d };
        let p = format!("{}/{name}", d.trim_end_matches('/'));
        is_exec(std::path::Path::new(&p))
    })
}

/// Re-runs `test` in the running test binary with RI_EQUIV_SKIP unset, then `env` applied
/// (`None` removes a variable), and panics unless it fails on a missing reference: a test whose
/// environment is gone must not pass by returning early.
#[track_caller]
pub fn assert_fails_when_missing(test: &str, env: &[(&str, Option<&str>)]) {
    let mut c = std::process::Command::new(std::env::current_exe().expect("test binary"));
    c.args([test, "--exact", "--test-threads=1", "--nocapture"])
        .env_remove("RI_EQUIV_SKIP");
    for (k, v) in env {
        match v {
            Some(v) => c.env(k, v),
            None => c.env_remove(k),
        };
    }
    let o = c.output().expect("re-run the test binary");
    let out = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(
        out.contains(&format!("test {test} ... FAILED"))
            && out.contains("equivalence reference missing"),
        "{test} did not fail on its missing environment {env:?}:\n{out}"
    );
}

fn skip_allowed() -> bool {
    skip_value(std::env::var_os("RI_EQUIV_SKIP").as_deref())
}

fn skip_value(v: Option<&std::ffi::OsStr>) -> bool {
    v.is_some_and(|v| v == "1")
}

/// Creates `path` as an executable script with `body`, for a test to run. install(1) writes it
/// in its own process: a writable descriptor of it in the test binary would be inherited by any
/// child a parallel test forks at that moment and stay open until that child's exec, and
/// running `path` meanwhile fails with ETXTBSY (execve(2)).
pub fn install_script(path: &std::path::Path, body: &str) {
    let mut src = path.as_os_str().to_owned();
    src.push(".src");
    std::fs::write(&src, body).expect("write script source");
    let st = std::process::Command::new("install")
        .args(["-m", "755"])
        .arg(&src)
        .arg(path)
        .status()
        .expect("run install(1)");
    assert!(st.success(), "install {}: {st}", path.display());
    std::fs::remove_file(&src).expect("remove script source");
}

/// `exec -a argv0 sleep infinity` as Flatpak runs an app: as PID 1 of new user, mount and PID
/// namespaces (bwrap's `--unshare-user --unshare-pid`), chrooted to a tmpfs root holding the
/// host's `/usr` and a `/.flatpak-info` naming `app`. `dir` is an empty directory for that
/// root. Killing the returned `unshare` kills the sandbox (`--kill-child`). Fails the test
/// (see [`equiv_missing`]) where unprivileged user namespaces are unavailable.
#[track_caller]
pub fn flatpak_sandbox(
    dir: &std::path::Path,
    app: &str,
    argv0: &str,
) -> Option<std::process::Child> {
    const SETUP: &str = r#"set -e
mount -t tmpfs tmpfs "$R"
mkdir "$R/usr" "$R/tmp"
mount --rbind /usr "$R/usr"
ln -s usr/bin "$R/bin"
ln -s usr/lib "$R/lib"
ln -s usr/lib64 "$R/lib64"
printf '[Application]\nname=%s\n' "$APP" > "$R/.flatpak-info"
exec chroot "$R" /bin/bash -c 'exec -a "$0" sleep infinity' "$ARGV0"
"#;
    let ok = std::process::Command::new("unshare")
        .args([
            "--user",
            "--map-root-user",
            "--mount",
            "--pid",
            "--fork",
            "true",
        ])
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        equiv_missing("unprivileged user namespaces (`unshare --user --map-root-user`)");
        return None;
    }
    std::process::Command::new("unshare")
        .args([
            "--user",
            "--map-root-user",
            "--mount",
            "--pid",
            "--fork",
            "--kill-child",
            "sh",
            "-c",
            SETUP,
        ])
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("R", dir)
        .env("APP", app)
        .env("ARGV0", argv0)
        .spawn()
        .ok()
}

const OWN_PROCESS: &str = "TESTENV_OWN_PROCESS";

/// libtest names a test by its path without the crate's name.
fn test_name(test: &str) -> &str {
    test.split_once("::").map_or(test, |(_, t)| t)
}

/// This process is the one [`run_in_own_process`] started for `test`.
pub fn in_own_process(test: &str) -> bool {
    std::env::var_os(OWN_PROCESS).is_some_and(|t| t == test_name(test))
}

/// The test binary set to run `test` (its full path) alone, where [`in_own_process`] holds:
/// for a test body that is a helper process the code under test starts and talks to.
pub fn own_process_command(test: &str) -> std::process::Command {
    let name = test_name(test);
    let mut c = std::process::Command::new(std::env::current_exe().expect("test binary"));
    c.args([name, "--exact", "--include-ignored", "--test-threads=1"])
        .env(OWN_PROCESS, name);
    c
}

/// Runs `test` (its full path, `concat!(module_path!(), "::name")`) in a process of its own
/// with `vars` set, as nextest runs every test, and asserts that it ran and passed there:
/// `std::env::set_var` is unsound while other threads run, and a libtest binary under
/// `cargo test` runs its tests on threads of one process, so a test passes the environment
/// it needs to that process instead. The test's body runs where [`in_own_process`] holds; the
/// files `vars` name are made here, before, and outlive that process. A `test` naming no test
/// fails: libtest exits 0 when its filter matches nothing, so its summary must count exactly
/// one test, passed.
#[track_caller]
pub fn run_in_own_process<K, V>(test: &str, vars: impl IntoIterator<Item = (K, V)>)
where
    K: AsRef<std::ffi::OsStr>,
    V: AsRef<std::ffi::OsStr>,
{
    run_in_own_process_env(test, vars, &[]);
}

/// [`run_in_own_process`], with the variables `unset` names also removed from that process's
/// environment (`Command::env_remove`).
#[track_caller]
pub fn run_in_own_process_env<K, V>(
    test: &str,
    vars: impl IntoIterator<Item = (K, V)>,
    unset: &[&str],
) where
    K: AsRef<std::ffi::OsStr>,
    V: AsRef<std::ffi::OsStr>,
{
    let exe = std::env::current_exe().expect("test binary");
    own_process(test, std::process::Command::new(exe), vars, unset);
}

/// [`run_in_own_process`] as root of a new user namespace (`unshare --user --map-root-user`):
/// what the test starts is owned by uid 0 there, for a client that trusts only a root-owned
/// service (libnm registers its secret agent only with a NetworkManager owned by root), while
/// the test itself runs as any user. Fails the test (see [`equiv_missing`]) where unprivileged
/// user namespaces are unavailable.
#[track_caller]
pub fn run_in_own_process_as_root<K, V>(test: &str, vars: impl IntoIterator<Item = (K, V)>)
where
    K: AsRef<std::ffi::OsStr>,
    V: AsRef<std::ffi::OsStr>,
{
    let userns = std::process::Command::new("unshare")
        .args(["--user", "--map-root-user", "true"])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !userns {
        equiv_missing("unprivileged user namespaces (`unshare --user --map-root-user`)");
        return;
    }
    let mut cmd = std::process::Command::new("unshare");
    cmd.args(["--user", "--map-root-user", "--"])
        .arg(std::env::current_exe().expect("test binary"));
    own_process(test, cmd, vars, &[]);
}

/// [`run_in_own_process`] in user and mount namespaces of its own (`unshare --user
/// --map-root-user --mount`) with an empty tmpfs on `/run`: for a test that serves a socket at
/// the fixed path a system helper is reached at (polkit's `/run/polkit/agent-helper.socket`)
/// without the host's. Fails the test (see [`equiv_missing`]) where unprivileged user and
/// mount namespaces are unavailable.
#[track_caller]
pub fn run_in_own_process_with_own_run<K, V>(test: &str, vars: impl IntoIterator<Item = (K, V)>)
where
    K: AsRef<std::ffi::OsStr>,
    V: AsRef<std::ffi::OsStr>,
{
    own_process_with_own(test, vars, "/run");
}

/// [`run_in_own_process_with_own_run`] with the empty tmpfs on `/etc`: for a test that writes
/// the files the system is configured by at fixed paths (`/etc/localtime`, `/etc/timezone`)
/// without the host's.
#[track_caller]
pub fn run_in_own_process_with_own_etc<K, V>(test: &str, vars: impl IntoIterator<Item = (K, V)>)
where
    K: AsRef<std::ffi::OsStr>,
    V: AsRef<std::ffi::OsStr>,
{
    own_process_with_own(test, vars, "/etc");
}

/// [`run_in_own_process`] in user and mount namespaces of its own with an empty tmpfs on `dir`.
#[track_caller]
fn own_process_with_own<K, V>(test: &str, vars: impl IntoIterator<Item = (K, V)>, dir: &str)
where
    K: AsRef<std::ffi::OsStr>,
    V: AsRef<std::ffi::OsStr>,
{
    let own = || {
        let mut cmd = std::process::Command::new("unshare");
        let mount = format!(r#"mount -t tmpfs tmpfs {dir} && exec "$0" "$@""#);
        cmd.args([
            "--user",
            "--map-root-user",
            "--mount",
            "--",
            "sh",
            "-c",
            &mount,
        ]);
        cmd
    };
    let ns = own()
        .arg("true")
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !ns {
        equiv_missing("unprivileged user and mount namespaces (`unshare --user --mount`)");
        return;
    }
    let mut cmd = own();
    cmd.arg(std::env::current_exe().expect("test binary"));
    own_process(test, cmd, vars, &[]);
}

/// Runs `cmd` (the test binary, maybe behind a wrapper) on `test` alone and asserts that it
/// ran and passed.
#[track_caller]
fn own_process<K, V>(
    test: &str,
    mut cmd: std::process::Command,
    vars: impl IntoIterator<Item = (K, V)>,
    unset: &[&str],
) where
    K: AsRef<std::ffi::OsStr>,
    V: AsRef<std::ffi::OsStr>,
{
    let name = test_name(test);
    for k in unset {
        cmd.env_remove(k);
    }
    let o = cmd
        .args([
            name,
            "--exact",
            "--include-ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(OWN_PROCESS, name)
        .envs(vars)
        .output()
        .expect("re-run the test binary");
    let (stdout, stderr) = (
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr),
    );
    print!("{stdout}");
    elog::eprint!("{stderr}");
    assert!(
        o.status.success() && ran_one_and_passed(&stdout),
        "{name} did not run and pass in its own process ({}):\n{stdout}{stderr}",
        o.status
    );
}

/// libtest's last summary line counts exactly one test, passed.
fn ran_one_and_passed(stdout: &str) -> bool {
    stdout
        .lines()
        .rev()
        .find(|l| l.starts_with("test result: "))
        .is_some_and(|l| {
            l.starts_with("test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured;")
        })
}

/// `hour:minute` of a fixed day in the machine's time zone, in ms since the epoch: the wall
/// clock a test gives the engine (`qml::builtins::set_clock`) when the time of day decides
/// something in it (the original's night light schedule). The QML then reads `hour:minute` at whatever
/// hour and in whatever zone the test runs. The day is in mid-January, when no zone switches
/// to or from summer time, so minutes added to it stay on the same offset. The zone is the
/// C library's (`TZ`, else /etc/localtime; zone files under `TZDIR` first), which is the
/// one the engine reads.
pub fn local_time_ms(hour: u32, minute: u32) -> f64 {
    // SAFETY: `libc::tm` is integers and a `tm_zone` pointer, for which all zeros (null) is
    // valid.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    (tm.tm_year, tm.tm_mon, tm.tm_mday) = (2026 - 1900, 0, 15);
    (tm.tm_hour, tm.tm_min, tm.tm_isdst) = (hour as _, minute as _, -1);
    // SAFETY: `tm` is an initialized local; mktime reads the zone anew (as `tzset`) and
    // normalizes `tm`.
    let secs = unsafe { libc::mktime(&mut tm) };
    let read = (tm.tm_mday, tm.tm_hour as u32, tm.tm_min as u32);
    assert_eq!(read, (15, hour, minute), "no such local time");
    secs as f64 * 1000.0
}

/// A TZif file (RFC 9636) of a zone always `ahead` seconds ahead of UTC, named `name`, whose
/// footer is the POSIX `rule`: a test's own zone, put in a directory that `TZDIR` names.
pub fn tzif(ahead: i32, name: &str, rule: &str) -> Vec<u8> {
    let counts = [0, 0, 0, 0, 1, name.len() as u32 + 1];
    let mut block = [b"TZif2".as_slice(), &[0; 15]].concat();
    block.extend(counts.iter().flat_map(|c| c.to_be_bytes()));
    block.extend(ahead.to_be_bytes());
    block.extend([0, 0]);
    block.extend(name.bytes().chain([0]));
    let mut file = block.repeat(2);
    file.extend(format!("\n{rule}\n").bytes());
    file
}

/// The environment that makes `home` the home of everything a test starts in its own process
/// ([`run_in_own_process`]): `HOME` and the XDG base directories the XDG Base Directory
/// Specification puts under it by default, set explicitly so the caller's own
/// `XDG_*_HOME` do not leak in.
pub fn home_vars(home: &std::path::Path) -> Vec<(&'static str, std::ffi::OsString)> {
    vec![
        ("HOME", home.into()),
        ("XDG_CONFIG_HOME", home.join(".config").into()),
        ("XDG_STATE_HOME", home.join(".local/state").into()),
        ("XDG_CACHE_HOME", home.join(".cache").into()),
        ("XDG_DATA_HOME", home.join(".local/share").into()),
    ]
}

mod fs;
mod held_feedback;
mod held_requests;
mod impulse_qt;
mod log;
pub mod pipewire;
mod proc;
mod sway;
pub mod wayland;
pub mod wire;
pub use fs::{DirWatch, fifo, release_fifo, wait_exists};
pub use held_feedback::HeldFeedback;
pub use held_requests::HeldRequests;
pub use impulse_qt::{impulse_qt_module, in_quickshells_words, tree_in_quickshells_words};
pub use log::Log;
pub use proc::{
    PidFd, Running, cat_copy, flatpak_sandbox_running, install_daemon, kill_pid_and_wait,
    output_with_descendants, refuse_new_threads, stderr_of, stderr_with, terminated, tool_path,
    wait_all_gone, wait_gone,
};
pub use sway::{Sway, SwayWindows};

/// Waits for `child` to exit and reaps it, sleeping in poll(2) on its pidfd (pidfd_open(2):
/// readable once the process terminated), never in a `try_wait` loop. No deadline of its own:
/// a hang is nextest's `slow-timeout` to end.
pub fn wait_exit(child: &mut std::process::Child) -> std::process::ExitStatus {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    // SAFETY: pidfd_open(2) takes a pid and flags and returns a new descriptor or -1. The child
    // is not reaped yet (only `child` reaps it), so its pid still names it.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, child.id() as libc::pid_t, 0) };
    assert!(
        fd >= 0,
        "pidfd_open({}): {}",
        child.id(),
        std::io::Error::last_os_error()
    );
    // SAFETY: `fd` is the new descriptor pidfd_open returned; nothing else owns it.
    let pidfd = unsafe { OwnedFd::from_raw_fd(fd as libc::c_int) };
    loop {
        let mut p = libc::pollfd {
            fd: pidfd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: poll(2) reads and writes only the one pollfd it is given.
        if unsafe { libc::poll(&mut p, 1, -1) } == 1 {
            return child.wait().expect("reap the exited child");
        }
        let e = std::io::Error::last_os_error();
        assert_eq!(e.kind(), std::io::ErrorKind::Interrupted, "poll: {e}");
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn own_process_gets_the_environment_it_is_given() {
        const TEST: &str = concat!(
            module_path!(),
            "::own_process_gets_the_environment_it_is_given"
        );
        if !super::in_own_process(TEST) {
            let home = std::path::Path::new("/nonexistent/testenv-home");
            let mut vars = super::home_vars(home);
            vars.push(("TESTENV_PROBE", "1".into()));
            super::run_in_own_process_env(TEST, vars, &["PATH"]);
            assert_eq!(std::env::var_os("TESTENV_PROBE"), None);
            return;
        }
        assert_eq!(std::env::var("TESTENV_PROBE").as_deref(), Ok("1"));
        assert_eq!(std::env::var_os("PATH"), None);
        assert_eq!(
            std::env::var("XDG_STATE_HOME").as_deref(),
            Ok("/nonexistent/testenv-home/.local/state")
        );
    }

    #[test]
    fn own_process_runs_the_test_with_its_vars() {
        const TEST: &str = concat!(module_path!(), "::own_process_runs_the_test_with_its_vars");
        if !super::in_own_process(TEST) {
            super::run_in_own_process(TEST, [("TESTENV_OWN_PROCESS_VAR", "set")]);
            return;
        }
        assert_eq!(
            std::env::var("TESTENV_OWN_PROCESS_VAR").as_deref(),
            Ok("set")
        );
    }

    #[test]
    fn own_process_fails_when_the_name_matches_no_test() {
        let r = std::panic::catch_unwind(|| {
            super::run_in_own_process(
                concat!(module_path!(), "::no_test_has_this_name"),
                [("TESTENV_OWN_PROCESS_VAR", "set")],
            )
        });
        assert!(r.is_err(), "a name matching no test passed");
    }

    #[test]
    fn own_process_fails_when_the_test_fails_there() {
        const TEST: &str = concat!(module_path!(), "::passes_except_in_its_own_process");
        let r = std::panic::catch_unwind(|| {
            super::run_in_own_process(TEST, [("TESTENV_OWN_PROCESS_VAR", "set")])
        });
        assert!(r.is_err(), "a test failing in its own process passed");
    }

    #[test]
    fn passes_except_in_its_own_process() {
        const TEST: &str = concat!(module_path!(), "::passes_except_in_its_own_process");
        assert!(
            !super::in_own_process(TEST),
            "fails where it runs on its own"
        );
    }

    #[test]
    fn the_summary_must_count_one_passed_test() {
        let ok = "running 1 test\ntest t ... ok\n\ntest result: ok. 1 passed; 0 failed; \
                  0 ignored; 0 measured; 9 filtered out; finished in 0.00s\n";
        assert!(super::ran_one_and_passed(ok));
        for out in [
            "running 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; \
             10 filtered out; finished in 0.00s\n",
            "running 1 test\ntest t ... ignored\n\ntest result: ok. 0 passed; 0 failed; \
             1 ignored; 0 measured; 9 filtered out; finished in 0.00s\n",
            "",
        ] {
            assert!(!super::ran_one_and_passed(out), "{out}");
        }
    }

    #[test]
    fn wait_exit_wakes_on_the_exit_and_reaps() {
        use std::os::unix::process::ExitStatusExt;
        let mut c = std::process::Command::new("sh")
            .args(["-c", "exit 7"])
            .spawn()
            .unwrap();
        assert_eq!(super::wait_exit(&mut c).code(), Some(7));
        let mut c = std::process::Command::new("sleep")
            .arg("infinity")
            .spawn()
            .unwrap();
        c.kill().unwrap();
        assert_eq!(super::wait_exit(&mut c).signal(), Some(libc::SIGKILL));
    }

    #[test]
    fn only_exactly_one_skips() {
        use std::ffi::OsStr;
        assert!(super::skip_value(Some(OsStr::new("1"))));
        for v in ["", "0", "yes", "true", "2", " 1", "1 "] {
            assert!(!super::skip_value(Some(OsStr::new(v))), "{v:?}");
        }
        assert!(!super::skip_value(None));
    }

    #[test]
    fn missing_tool_fails_unless_skipping() {
        let missing = "ri-testenv-no-such-tool";
        for r in [
            std::panic::catch_unwind(|| super::require(missing)),
            std::panic::catch_unwind(|| super::require_all(&["sh", missing])),
        ] {
            if super::skip_allowed() {
                assert_eq!(r.ok(), Some(false));
            } else {
                let msg = r.unwrap_err();
                let msg = msg.downcast_ref::<String>().unwrap();
                assert!(
                    msg.contains(missing) && msg.contains("RI_EQUIV_SKIP=1"),
                    "{msg}"
                );
            }
        }
        assert!(super::require("sh"));
        assert!(super::require_all(&["sh", "env"]));
    }

    /// A ping that came before the wait started (from another thread, as a runtime's helper
    /// threads send them) is what the wait returns, and it is consumed there.
    #[test]
    fn a_ping_sent_before_the_wait_is_seen() {
        let w = super::Wakeups::new();
        let ping = w.ping();
        std::thread::spawn(ping).join().unwrap();
        let woke = w.wait(None, None);
        assert_eq!(
            woke,
            super::Woke {
                ping: true,
                fd: false
            }
        );
        assert!(!w.wait(None, Some(std::time::Duration::ZERO)).any());
    }
}

/// What [`Wakeups::wait`] saw ready.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Woke {
    /// A ping arrived since the last wait.
    pub ping: bool,
    /// The extra fd is readable.
    pub fd: bool,
}

impl Woke {
    pub fn any(self) -> bool {
        self.ping || self.fd
    }
}

/// A test's end of a runtime waker: [`Wakeups::ping`] is what the runtime calls when work
/// for its loop arrives from another thread (`waker.set_ping`), and [`Wakeups::wait`] blocks
/// until that happens or a given fd (the runtime's epoll set) is readable, the way the shell's
/// own loop sleeps. A ping that arrives between two waits is kept, so none is lost.
pub struct Wakeups {
    rx: std::os::unix::net::UnixStream,
    tx: std::sync::Arc<std::os::unix::net::UnixStream>,
}

impl Default for Wakeups {
    fn default() -> Self {
        Self::new()
    }
}

impl Wakeups {
    pub fn new() -> Wakeups {
        let (rx, tx) = std::os::unix::net::UnixStream::pair().expect("socketpair");
        rx.set_nonblocking(true).expect("nonblocking wakeup socket");
        tx.set_nonblocking(true).expect("nonblocking wakeup socket");
        Wakeups {
            rx,
            tx: std::sync::Arc::new(tx),
        }
    }

    /// The function to install as the runtime's ping. A full socket already holds a pending
    /// wakeup, so a write that would block is dropped.
    pub fn ping(&self) -> impl Fn() + Send + Sync + 'static {
        let tx = self.tx.clone();
        move || {
            use std::io::Write;
            let _ = (&*tx).write(&[1]);
        }
    }

    /// Blocks until a ping or `fd` is readable, or `timeout` passes (`None`: no limit,
    /// `Some(ZERO)`: only looks), and consumes the pings.
    pub fn wait(
        &self,
        fd: Option<std::os::fd::RawFd>,
        timeout: Option<std::time::Duration>,
    ) -> Woke {
        use std::io::Read;
        use std::os::fd::AsRawFd;
        let mut fds = [
            libc::pollfd {
                fd: self.rx.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: fd.unwrap_or(-1),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let ms = timeout.map_or(-1, |t| {
            i32::try_from(t.as_nanos().div_ceil(1_000_000)).unwrap_or(i32::MAX)
        });
        loop {
            // SAFETY: poll(2) reads and writes only the two pollfds it is given; a negative fd
            // is ignored.
            let r = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, ms) };
            if r >= 0 {
                break;
            }
            let e = std::io::Error::last_os_error();
            assert_eq!(e.kind(), std::io::ErrorKind::Interrupted, "poll: {e}");
        }
        let ping = fds[0].revents & libc::POLLIN != 0;
        if ping {
            let mut b = [0u8; 64];
            while matches!((&self.rx).read(&mut b), Ok(n) if n > 0) {}
        }
        Woke {
            ping,
            fd: fds[1].revents != 0,
        }
    }
}
