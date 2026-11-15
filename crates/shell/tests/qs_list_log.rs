//! `qs list` / `qs log` equivalence: the native `qs` (impulse-shell invoked as `qs`) and the original
//! Quickshell must print the same stdout/stderr and exit with the same code over one runtime
//! directory of live, dead and other-display instances, native- and Qt-written `log.qslog` files,
//! and malformed logs. The original is `$RI_REF_QS` or `quickshell` on PATH; without it the test
//! fails (`RI_EQUIV_SKIP=1` skips knowingly).

use std::fs::File;
use std::io::Write;
use std::os::unix::fs::symlink;
use std::os::unix::io::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use shell::rilog::{Encoder, Filter, Level, Msg};

const NATIVE: &str = env!("CARGO_BIN_EXE_impulse-shell");

fn reference() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("RI_REF_QS") {
        return Some(PathBuf::from(p));
    }
    let me = std::fs::canonicalize(NATIVE).ok();
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|d| d.join("quickshell"))
        .find(|p| p.is_file() && std::fs::canonicalize(p).ok() != me)
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    let u: Vec<u16> = s.encode_utf16().collect();
    out.extend(((u.len() * 2) as u32).to_be_bytes());
    for c in u {
        out.extend(c.to_be_bytes());
    }
}

fn md5_hex(s: &str) -> String {
    qml::builtins::md5_hex(s.as_bytes())
}

fn write_lock(f: &File) {
    let fl = libc::flock {
        l_type: libc::F_WRLCK as _,
        l_whence: libc::SEEK_SET as _,
        l_start: 0,
        l_len: 0,
        l_pid: 0,
    };
    // SAFETY: F_SETLK on the open file takes a pointer to an initialized flock.
    assert_eq!(unsafe { libc::fcntl(f.as_raw_fd(), libc::F_SETLK, &fl) }, 0);
}

struct Spec<'a> {
    id: &'a str,
    config: &'a str,
    /// (julian day, ms of day, Qt::TimeSpec, offset seconds)
    time: (i64, u32, u8, i32),
    pid: i32,
    display: &'a str,
    live: bool,
}

/// Register an instance the way Quickshell does; a live one stays write-locked by this process.
fn instance(rt: &Path, s: &Spec) -> Option<File> {
    let path_id = md5_hex(s.config);
    let dir = rt.join("by-id").join(s.id);
    let shell = rt.join("by-shell").join(&path_id);
    for d in [&dir, &shell, &rt.join("by-pid"), &rt.join("by-path")] {
        std::fs::create_dir_all(d).unwrap();
    }
    let mut b = Vec::new();
    for x in [s.id, s.config, &path_id, "org.quickshell"] {
        put_str(&mut b, x);
    }
    let (jd, ms, spec, off) = s.time;
    b.extend(jd.to_be_bytes());
    b.extend(ms.to_be_bytes());
    b.push(spec);
    if spec == 2 {
        b.extend(off.to_be_bytes());
    }
    b.extend(s.pid.to_be_bytes());
    put_str(&mut b, s.display);
    let mut f = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(dir.join("instance.lock"))
        .unwrap();
    f.write_all(&b).unwrap();
    let _ = symlink(&dir, shell.join(s.id));
    let _ = symlink(&dir, rt.join("by-pid").join(s.pid.to_string()));
    let _ = symlink(&shell, rt.join("by-path").join(&path_id));
    // The native shell finds the same config under its own name for the directory.
    let own = md5_hex(&s.config.replace("/quickshell/", "/rustshell/"));
    let _ = symlink(&shell, rt.join("by-path").join(own));
    s.live.then(|| {
        write_lock(&f);
        f
    })
}

fn msg(ty: Level, cat: &str, body: &str) -> Msg {
    Msg {
        ty,
        category: cat.into(),
        body: body.into(),
    }
}

fn sample_messages() -> Vec<(Msg, i64)> {
    let t = 1_767_312_000_000; // 2026-01-02 00:00:00 UTC
    vec![
        (
            msg(Level::Info, "default", "Launching config: \"/x/shell.qml\""),
            t + 120,
        ),
        (msg(Level::Info, "default", "Configuration Loaded"), t + 300),
        (msg(Level::Debug, "qml", "hello 1 wörld"), t + 1_400),
        (msg(Level::Info, "qml", "info msg"), t + 1_400),
        (msg(Level::Warn, "qml", "warn msg"), t + 2_000),
        (msg(Level::Critical, "qml", "error msg"), t + 2_000),
        (msg(Level::Debug, "qml", "hello 1 wörld"), t + 9_000),
        (msg(Level::Debug, "qml", "hello 1 wörld"), t + 60_000),
        (msg(Level::Debug, "qml", "line1\nline2\ttab"), t + 60_000),
        (
            msg(Level::Warn, "quickshell.service.x", "svc warn"),
            t + 60_000 + 0xfff_f000,
        ),
        (
            msg(Level::Info, "quickshell.bare", "bare text"),
            t + 60_000 + 0xfff_f000,
        ),
        (
            msg(Level::Debug, "qml", &"x".repeat(70_000)),
            t + 200_000_000,
        ),
        (
            msg(Level::Info, "q\u{e9}l", "latin1 category"),
            t + 200_000_100,
        ),
        (msg(Level::Fatal, "qml", "fatal msg"), t + 200_000_200),
        (msg(Level::Info, "qml", "after fatal"), t + 200_001_000),
    ]
}

fn encode(msgs: &[(Msg, i64)]) -> Vec<u8> {
    let mut b = Vec::new();
    Encoder::header(&mut b);
    let mut e = Encoder::default();
    for (m, ms) in msgs {
        e.write(&mut b, m, *ms, |c| Filter {
            debug: c != b"quickshell.service.x",
            ..Filter::default()
        });
    }
    b
}

struct Env {
    vars: Vec<(String, String)>,
}

impl Env {
    fn run(
        &self,
        bin: &Path,
        args: &[&str],
        extra: &[(&str, &str)],
    ) -> (String, String, Option<i32>) {
        let mut c = Command::new(bin);
        c.arg0("qs").args(args).env_clear().stdin(Stdio::null());
        for (k, v) in &self.vars {
            c.env(k, v);
        }
        for (k, v) in extra {
            c.env(k, v);
        }
        let o = c.output().unwrap();
        (
            String::from_utf8_lossy(&o.stdout).into_owned(),
            String::from_utf8_lossy(&o.stderr).into_owned(),
            o.status.code(),
        )
    }

    /// Compare one invocation; retried while the outputs differ, since `qs list` prints
    /// running times that tick between the two runs.
    fn same(&self, qt: &Path, args: &[&str], extra: &[(&str, &str)]) {
        let mut last = None;
        for _ in 0..5 {
            let want = self.run(qt, args, extra);
            let (out, err, code) = self.run(Path::new(NATIVE), args, extra);
            // The native shell names the directories by its own name for them.
            let theirs = |s: String| s.replace("/rustshell/", "/quickshell/");
            let got = (theirs(out), theirs(err), code);
            if want == got {
                return;
            }
            last = Some((want, got));
        }
        let (want, got) = last.unwrap();
        panic!(
            "qs {args:?} {extra:?} differs\n--- quickshell ---\n{want:?}\n--- native ---\n{got:?}"
        );
    }
}

#[test]
fn list_and_log_match_quickshell() {
    let Some(qt) = reference() else {
        testenv::unavailable("quickshell is not installed (or set RI_REF_QS)");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let rt = root.join("rt");
    let cfg = root.join("cfg");
    let t_dir = cfg.join("quickshell/t");
    std::fs::create_dir_all(&t_dir).unwrap();
    // The native shell reads the same directories under its own name.
    std::fs::create_dir_all(&rt).unwrap();
    for d in [&cfg, &rt] {
        std::os::unix::fs::symlink("quickshell", d.join("rustshell")).unwrap();
    }
    std::fs::write(t_dir.join("shell.qml"), "import Quickshell\nShellRoot {}\n").unwrap();
    let t_cfg = t_dir.join("shell.qml").to_string_lossy().into_owned();
    let mut vars = vec![
        (
            "XDG_RUNTIME_DIR".to_string(),
            rt.to_string_lossy().into_owned(),
        ),
        ("XDG_CONFIG_HOME".into(), cfg.to_string_lossy().into_owned()),
        (
            "XDG_CONFIG_DIRS".into(),
            root.join("none").to_string_lossy().into_owned(),
        ),
        ("HOME".into(), root.to_string_lossy().into_owned()),
        ("WAYLAND_DISPLAY".into(), "wl-test".into()),
        ("QT_QPA_PLATFORM".into(), "offscreen".into()),
        // Qt warns on stdout about a non-UTF-8 locale.
        ("LANG".into(), "C.UTF-8".into()),
        ("PATH".into(), std::env::var("PATH").unwrap_or_default()),
    ];
    if Path::new("/usr/share/zoneinfo/Europe/Moscow").exists() {
        vars.push(("TZ".into(), "Europe/Moscow".into()));
    }
    let env = Env { vars };

    let jd = 2_461_043; // 2026-01-02
    let specs = [
        Spec {
            id: "a1live",
            config: &t_cfg,
            time: (jd, 3_600_000, 0, 0),
            pid: 41001,
            display: "wayland/wl-test",
            live: true,
        },
        Spec {
            id: "b2utc",
            config: &t_cfg,
            time: (jd, 7_200_500, 1, 0),
            pid: 41002,
            display: "wayland/other",
            live: true,
        },
        Spec {
            id: "c3off",
            config: &t_cfg,
            time: (jd - 1, 82_800_000, 2, 10_800),
            pid: 41003,
            display: "wayland/wl-test",
            live: true,
        },
        Spec {
            id: "d4dead",
            config: &t_cfg,
            time: (jd - 3, 1_000, 0, 0),
            pid: 41004,
            display: "wayland/wl-test",
            live: false,
        },
        Spec {
            id: "e5dead",
            config: "/elsewhere/sh\"ell.qml",
            time: (jd - 2, 5_000, 1, 0),
            pid: 41005,
            display: "x11/:1",
            live: false,
        },
        Spec {
            id: "f6dead",
            config: &t_cfg,
            time: (jd - 1, 5_000, 0, 0),
            pid: 41006,
            display: "wayland/wl-test",
            live: false,
        },
        Spec {
            id: "g7zero",
            config: "/other/shell.qml",
            time: (jd, 0, 2, 0),
            pid: 41007,
            display: "unk",
            live: true,
        },
    ];
    let qrt = rt.join("quickshell");
    let _locks: Vec<File> = specs.iter().filter_map(|s| instance(&qrt, s)).collect();

    let log = encode(&sample_messages());
    for id in ["a1live", "d4dead", "f6dead"] {
        std::fs::write(qrt.join("by-id").join(id).join("log.qslog"), &log).unwrap();
    }
    let logs = root.join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    let file = |name: &str, data: &[u8]| {
        let p = logs.join(name);
        std::fs::write(&p, data).unwrap();
        p.to_string_lossy().into_owned()
    };
    let good = file("good.qslog", &log);
    let truncated = file("truncated.qslog", &log[..log.len() - 5]);
    let mut garbage = log.clone();
    garbage.extend([0x03, 0x01, 0x02, 0x41, 0x42]);
    let garbage = file("garbage.qslog", &garbage);
    let mut partial = log.clone();
    partial.extend([0xff, 0x01]);
    let partial = file("partial.qslog", &partial);
    let empty = file("empty.qslog", b"");
    let version = file("version.qslog", b"\x07rest");
    let missing = logs.join("missing.qslog").to_string_lossy().into_owned();
    let t_path = t_dir.to_string_lossy().into_owned();

    let cases: Vec<Vec<&str>> = vec![
        vec!["list", "-a"],
        vec!["list", "--all", "--show-dead"],
        vec!["list", "-a", "--show-dead", "--no-color"],
        vec!["list", "-a", "--show-dead", "-n"],
        vec!["list", "-a", "-j"],
        vec!["list", "-a", "--show-dead", "--json", "--newest"],
        vec!["list", "-c", "t"],
        vec!["list", "-c", "t", "--show-dead"],
        vec!["list", "-c", "t", "--any-display", "--show-dead"],
        vec!["list", "-p", &t_path, "-j"],
        vec!["list", "-c", "missing"],
        vec!["list", "-p", "/nonexistent/x.qml"],
        vec!["list", "-c", "t", "-a"],
        vec!["log", &good],
        vec!["log", &good, "--no-color"],
        vec!["log", &good, "--log-times"],
        vec!["log", &good, "--log-times", "--no-color"],
        vec!["log", &good, "-t", "3"],
        vec!["log", &good, "--tail", "100"],
        vec!["log", &good, "-t", "0"],
        vec!["log", &good, "-t", "abc"],
        vec!["log", &good, "-r", "qml.debug=false"],
        vec!["log", &good, "-r", "*.warning=false; qml.info = false"],
        vec!["log", &good, "-r", "quickshell.*=true;qml.critical=false"],
        vec!["log", &good, "-r", "bogus;a*b=true;x=y=z;q=maybe"],
        vec!["log", &good, "-f"],
        vec!["log", &truncated],
        vec!["log", &garbage, "--no-color"],
        vec!["log", &partial],
        vec!["log", &empty],
        vec!["log", &version],
        vec!["log", &missing],
        vec!["log", "-i", "a1"],
        vec!["log", "-i", "d4", "-t", "2"],
        vec!["log", "-i", "zz"],
        vec!["log", "-i", "b2"],
        vec!["log", "--pid", "41001", "--no-color"],
        vec!["log", "--pid", "41006", "-t", "1"],
        vec!["log", "--pid", "99999"],
        vec!["log", "-c", "t", "-t", "2"],
        vec!["log", "-c", "missing"],
        vec!["list", "-a", "--any-display"],
        vec!["log", &good, "-i", "a1"],
        vec!["log", &good, "-c", "t"],
        vec!["log", &good, "-n"],
        vec!["log", "-i", "a1", "-c", "t"],
        vec!["log", &good, "-i", "a1", "-c", "t"],
    ];
    for args in &cases {
        env.same(&qt, args, &[]);
    }
    env.same(&qt, &["list", "-a", "--show-dead"], &[("NO_COLOR", "1")]);
    env.same(&qt, &["log", &good], &[("NO_COLOR", "1")]);

    follow_matches(&env, &qt, &logs);
    follow_across_rotation(&env, &logs);
    qt_written_matches(&env, &qt, root);
}

/// `qs log -f` on a log whose writer still holds its lock: both print the appended messages and
/// exit once the lock is released. The messages are appended once the follower printed the
/// ones already there (it opened the log, and the held lock keeps it following); it must still
/// be running after them, and exit only once the lock goes.
fn follow_matches(env: &Env, qt: &Path, logs: &Path) {
    let msgs = sample_messages();
    let run = |bin: &Path| {
        let path = logs.join("follow.qslog");
        let mut f = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .unwrap();
        write_lock(&f);
        let mut enc = Encoder::default();
        let mut b = Vec::new();
        Encoder::header(&mut b);
        for (m, ms) in &msgs[..4] {
            enc.write(&mut b, m, *ms, |_| Filter::default());
        }
        assert_eq!(
            msgs[3].0.body, b"info msg",
            "the last message already there"
        );
        f.write_all(&b).unwrap();
        let mut c = Command::new(bin);
        c.arg0("qs")
            .args(["log", "-f", "--no-color", &path.to_string_lossy()])
            .env_clear();
        for (k, v) in &env.vars {
            c.env(k, v);
        }
        let (out_r, out_w) = std::io::pipe().unwrap();
        let (err_r, err_w) = std::io::pipe().unwrap();
        let mut child = c.stdout(out_w).stderr(err_w).spawn().unwrap();
        drop(c);
        let (out, err) = (testenv::Log::capture(out_r), testenv::Log::capture(err_r));
        out.wait(|o| o.contains("info msg"))
            .unwrap_or_else(|o| panic!("{bin:?} never printed the existing log: {o:?}"));
        for (m, ms) in &msgs[4..9] {
            let mut b = Vec::new();
            enc.write(&mut b, m, *ms, |_| Filter::default());
            f.write_all(&b).unwrap();
        }
        // SAFETY: siginfo_t is plain data; all zeros is a valid value.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        // SAFETY: waitid(2) with WNOHANG|WNOWAIT only reports (without reaping) whether the
        // child, which `child` alone reaps, has exited; it writes only `info`.
        let r = unsafe {
            libc::waitid(
                libc::P_PID,
                child.id(),
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        assert_eq!(r, 0, "waitid: {}", std::io::Error::last_os_error());
        // SAFETY: waitid filled `info`; with WNOHANG si_pid stays 0 while the child runs.
        let exited = unsafe { info.si_pid() } != 0;
        assert!(
            !exited,
            "{bin:?}: qs log -f exited while the log was locked"
        );
        drop(f);
        let status = testenv::wait_exit(&mut child);
        (out.until_closed(), err.until_closed(), status.code())
    };
    let want = run(qt);
    let got = run(Path::new(NATIVE));
    assert_eq!(want, got, "qs log -f differs");
    assert!(
        want.0.contains("error msg") && want.0.contains("line2"),
        "{want:?}"
    );
}

/// `impulse-shell log -f` ends by SIGINT, SIGTERM and SIGHUP (Ctrl+C, `kill`, a closed terminal), as
/// any program that handles none of them: it runs no main loop that reads the shell's signal
/// pipe, so the shell's handlers must not be installed in it. The signal is sent once the
/// follower printed the log's one message (it follows: the writer's lock is held), and the lock
/// is released only after kill(2) returned: a follower the signal did not end exits with status
/// 0 then, instead of leaving the test waiting.
#[test]
fn log_follow_ends_by_a_termination_signal() {
    use std::os::unix::process::ExitStatusExt;
    let tmp = tempfile::tempdir().unwrap();
    for sig in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
        let path = tmp.path().join(format!("follow-{sig}.qslog"));
        let mut f = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .unwrap();
        write_lock(&f);
        let mut b = Vec::new();
        Encoder::header(&mut b);
        Encoder::default().write(
            &mut b,
            &msg(Level::Info, "qml", "info msg"),
            1_767_312_000_000,
            |_| Filter::default(),
        );
        f.write_all(&b).unwrap();
        let mut c = Command::new(NATIVE);
        c.args(["log", "-f", "--no-color", &path.to_string_lossy()])
            .stdin(Stdio::null());
        // SAFETY: signal(2) is async-signal-safe; the closure touches nothing else. A test
        // runner that ignores one of the signals would hand that on to the follower.
        let default_action = move || {
            // SAFETY: signal(2) with SIG_DFL installs no handler and is async-signal-safe.
            unsafe { libc::signal(sig, libc::SIG_DFL) };
            Ok(())
        };
        // SAFETY: `default_action` runs in the forked child before exec and only calls
        // signal(2), which is async-signal-safe.
        unsafe { c.pre_exec(default_action) };
        let (out_r, out_w) = std::io::pipe().unwrap();
        let mut child = c.stdout(out_w).spawn().unwrap();
        drop(c);
        let out = testenv::Log::capture(out_r);
        out.wait(|o| o.contains("info msg"))
            .unwrap_or_else(|o| panic!("impulse-shell log -f never printed the log: {o:?}"));
        // SAFETY: kill(2) takes plain integers; the child is not reaped yet, so the pid is its.
        // SAFETY: kill(2) takes plain integers; `child` is this test's unreaped child.
        let sent = unsafe { libc::kill(child.id() as libc::pid_t, sig) };
        assert_eq!(sent, 0, "{}", std::io::Error::last_os_error());
        drop(f);
        let status = testenv::wait_exit(&mut child);
        assert_eq!(
            status.signal(),
            Some(sig),
            "impulse-shell log -f after signal {sig}: {status}"
        );
    }
}

/// Starts `bin log -f` on a new log its writer (the returned file) holds locked, with one
/// message in it, reads that message from the follower's output and closes the output, as
/// `head -1` does after its line; then appends a second message, which the follower has
/// nobody to print to.
fn follow_with_closed_output(bin: &Path, dir: &Path) -> (File, std::process::Child) {
    use std::io::BufRead;
    let path = dir.join("closed.qslog");
    let mut f = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .unwrap();
    write_lock(&f);
    let mut enc = Encoder::default();
    let mut b = Vec::new();
    Encoder::header(&mut b);
    let first = msg(Level::Info, "qml", "info msg");
    enc.write(&mut b, &first, 1_767_312_000_000, |_| Filter::default());
    f.write_all(&b).unwrap();
    let mut c = Command::new(bin);
    c.arg0("qs")
        .args(["log", "-f", "--no-color", &path.to_string_lossy()])
        .env_clear()
        .env("XDG_RUNTIME_DIR", dir)
        .env("HOME", dir)
        .env("QT_QPA_PLATFORM", "offscreen")
        .env("LANG", "C.UTF-8")
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .stdin(Stdio::null());
    let (out_r, out_w) = std::io::pipe().unwrap();
    let child = c.stdout(out_w).spawn().unwrap();
    drop(c);
    let mut out = std::io::BufReader::new(out_r);
    let mut line = String::new();
    out.read_line(&mut line).unwrap();
    assert!(line.contains("info msg"), "{bin:?} printed {line:?}");
    drop(out);
    b.clear();
    let next = msg(Level::Warn, "qml", "warn msg");
    enc.write(&mut b, &next, 1_767_312_001_000, |_| Filter::default());
    f.write_all(&b).unwrap();
    (f, child)
}

/// `qs log -f` whose output is closed (`qs log -f | head -1`) ends as Quickshell's does: by
/// SIGPIPE at the next message it writes, not with success once it has written them to
/// nobody. The writer lets go of the log after the message, so each follower writes it
/// whether or not it was watching the log yet when it was appended.
#[test]
fn log_follow_with_its_output_closed_ends_as_quickshells() {
    use std::os::unix::process::ExitStatusExt;
    let Some(qt) = reference() else {
        testenv::unavailable("quickshell is not installed (or set RI_REF_QS)");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let run = |bin: &Path| {
        let (f, mut child) = follow_with_closed_output(bin, tmp.path());
        drop(f);
        let status = testenv::wait_exit(&mut child);
        (status.code(), status.signal())
    };
    let want = run(&qt);
    assert_eq!(want, (None, Some(libc::SIGPIPE)), "quickshell");
    assert_eq!(run(Path::new(NATIVE)), want);
}

/// `impulse-shell log -f | head -1` does not wait for the shell that writes the log to exit: the
/// follower is waited for while the writer still holds the log's lock, so one that goes on
/// following hangs here. It ends at the message appended after its output closed, also when
/// that was appended before it watched the log.
#[test]
fn log_follow_with_its_output_closed_ends_while_the_log_is_written() {
    use std::os::unix::process::ExitStatusExt;
    let tmp = tempfile::tempdir().unwrap();
    let (f, mut child) = follow_with_closed_output(Path::new(NATIVE), tmp.path());
    let status = testenv::wait_exit(&mut child);
    drop(f);
    assert_eq!(status.signal(), Some(libc::SIGPIPE), "{status}");
}

/// impulse-shell rotates its log (Quickshell does not): the next file, locked and with its header, is
/// renamed over the path while the old one, kept as `<name>.1`, is still locked. `qs log -f`
/// reads the old file to its end and follows the new one until its writer lets go of it.
fn follow_across_rotation(env: &Env, logs: &Path) {
    let msgs = sample_messages();
    let path = logs.join("rotate.qslog");
    let log_file = |p: &Path, msgs: &[(Msg, i64)]| {
        let mut f = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(p)
            .unwrap();
        write_lock(&f);
        let mut enc = Encoder::default();
        let mut b = Vec::new();
        Encoder::header(&mut b);
        for (m, ms) in msgs {
            enc.write(&mut b, m, *ms, |_| Filter::default());
        }
        f.write_all(&b).unwrap();
        (f, enc)
    };
    let (old, _) = log_file(&path, &msgs[..4]);
    let mut c = Command::new(NATIVE);
    c.arg0("qs")
        .args(["log", "-f", "--no-color", &path.to_string_lossy()])
        .env_clear();
    for (k, v) in &env.vars {
        c.env(k, v);
    }
    let (out_r, out_w) = std::io::pipe().unwrap();
    let mut child = c.stdout(out_w).stderr(Stdio::null()).spawn().unwrap();
    drop(c);
    let out = testenv::Log::capture(out_r);
    out.wait(|o| o.contains("info msg"))
        .unwrap_or_else(|o| panic!("qs log -f never printed the existing log: {o:?}"));
    let staged = logs.join("rotate.qslog.new");
    let (mut new, mut enc) = log_file(&staged, &msgs[4..6]);
    std::fs::hard_link(&path, logs.join("rotate.qslog.1")).unwrap();
    std::fs::rename(&staged, &path).unwrap();
    drop(old);
    assert_eq!(msgs[5].0.body, b"error msg", "the new file's last message");
    out.wait(|o| o.contains("error msg"))
        .unwrap_or_else(|o| panic!("qs log -f stopped at the rotation: {o:?}"));
    for (m, ms) in &msgs[6..9] {
        let mut b = Vec::new();
        enc.write(&mut b, m, *ms, |_| Filter::default());
        new.write_all(&b).unwrap();
    }
    drop(new);
    let status = testenv::wait_exit(&mut child);
    let printed = out.until_closed();
    assert_eq!(status.code(), Some(0), "{printed}");
    let read = |b: &Path| {
        let o = Command::new(NATIVE)
            .arg0("qs")
            .args(["log", "--no-color", &b.to_string_lossy()])
            .env_clear()
            .envs(env.vars.iter().map(|(k, v)| (k, v)))
            .output()
            .unwrap();
        String::from_utf8(o.stdout).unwrap()
    };
    // Each record once, the old file's then the new one's.
    let whole = read(&logs.join("rotate.qslog.1")) + &read(&path);
    assert_eq!(printed, whole);
}

/// A log and instance lock written by Quickshell itself read back by both.
fn qt_written_matches(env: &Env, qt: &Path, root: &Path) {
    let rt = root.join("qt-rt");
    // The native shell reads what Quickshell writes there under its own name.
    std::fs::create_dir_all(&rt).unwrap();
    std::os::unix::fs::symlink("quickshell", rt.join("rustshell")).unwrap();
    let dir = root.join("cfg/quickshell/w");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("shell.qml"),
        r#"import QtQuick
import Quickshell
ShellRoot {
    Timer {
        interval: 200; running: true
        onTriggered: {
            console.log("hello", 1, "wörld");
            console.info("info msg");
            console.warn("warn msg");
            console.error("error msg");
            console.debug("debug msg");
            for (let i = 0; i < 20; i++) console.log("rep " + (i % 3));
            console.log("x".repeat(300));
            console.log("line1\nline2\ttab");
            Qt.quit();
        }
    }
}
"#,
    )
    .unwrap();
    let mut vars = env.vars.clone();
    for v in &mut vars {
        if v.0 == "XDG_RUNTIME_DIR" {
            v.1 = rt.to_string_lossy().into_owned();
        }
    }
    let env = Env { vars };
    let mut c = Command::new(qt);
    c.args(["-c", "w"])
        .env_clear()
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (k, v) in &env.vars {
        c.env(k, v);
    }
    let mut child = c.spawn().unwrap();
    testenv::wait_exit(&mut child);
    let ids: Vec<String> = std::fs::read_dir(rt.join("quickshell/by-id"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(ids.len(), 1, "{ids:?}");
    // The native shell finds the same config under its own name for the directory.
    let by_path = rt.join("quickshell/by-path");
    let theirs = dir.join("shell.qml").to_string_lossy().into_owned();
    let _ = symlink(
        by_path.join(md5_hex(&theirs)),
        by_path.join(md5_hex(&theirs.replace("/quickshell/", "/rustshell/"))),
    );
    let qslog = rt.join("quickshell/by-id").join(&ids[0]).join("log.qslog");
    let qslog = qslog.to_string_lossy().into_owned();
    for args in [
        vec!["log", qslog.as_str()],
        vec!["log", &qslog, "--no-color", "-r", "quickshell.*.debug=true"],
        vec!["log", "-c", "w", "-t", "4"],
        vec!["log", "-i", &ids[0][..3], "--log-times"],
        vec!["list", "-c", "w", "--show-dead"],
        vec!["list", "-a", "--show-dead", "--json"],
    ] {
        env.same(qt, &args, &[]);
    }
}
