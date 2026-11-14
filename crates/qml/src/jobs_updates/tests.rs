// The answers against the commands the original ran before. `which` runs against a stub program on
// PATH. `checkupdates | wc -l` runs the real pacman-contrib script against
// stub (and then real) `pacman`/`pacman-conf`/`fakeroot`, and natively. The count, the
// pacman calls and their stderr are the same; the check's own error line is its own.
use super::*;
use crate::svc::Stop;
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

/// The commands the original ran before `UpdatesProbe` / `UpdatesCount`, the reference the jobs match.
const WHICH_CHECKUPDATES: &[&str] = &["which", "checkupdates"];
const COUNT_UPDATES: &[&str] = &["bash", "-c", "checkupdates | wc -l"];

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("qml-updwarp-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn stub(dir: &Path, name: &str, script: &str, exec: bool) {
    let p = dir.join(name);
    let body = format!("#!/bin/sh\n{script}\n");
    if exec {
        crate::testenv::install_script(&p, &body);
    } else {
        std::fs::write(&p, body).unwrap();
    }
}

/// A named pipe at `p`: a stub that writes to it blocks until the test opens it to read.
fn fifo(p: &Path) {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(p.as_os_str().as_bytes()).unwrap();
    // SAFETY: mkfifo(3) reads the NUL-terminated path `c` points to.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0, "mkfifo");
}

fn path_with(dir: &Path) -> OsString {
    format!("{}:/usr/bin:/bin", dir.display()).into()
}

fn real(argv: &[&str], path: &OsStr) -> Out {
    real_env(argv, path, &HashMap::new())
}

fn real_env(argv: &[&str], path: &OsStr, env: &HashMap<&str, OsString>) -> Out {
    let mut c = Command::new(argv[0]);
    c.args(&argv[1..]).env("PATH", path).stdin(Stdio::null());
    for k in ["CHECKUPDATES_DB", "TMPDIR"] {
        match env.get(k) {
            Some(v) => c.env(k, v),
            None => c.env_remove(k),
        };
    }
    let o = c.output().unwrap();
    Out {
        out: o.stdout,
        err: o.stderr,
        code: o.status.code().unwrap_or(-1),
    }
}

/// stderr without the line the check itself adds when it fails (`==> ERROR: …` from the
/// script, `updates: …` from the native check).
fn passed_through(err: &[u8]) -> String {
    String::from_utf8_lossy(err)
        .lines()
        .filter(|l| !l.starts_with("==> ERROR: ") && !l.starts_with("updates: "))
        .map(|l| format!("{l}\n"))
        .collect()
}

fn text(o: &Out) -> (String, String, i32) {
    (
        String::from_utf8_lossy(&o.out).into(),
        passed_through(&o.err),
        o.code,
    )
}

/// The real checkupdates script, or a failure (a skip with RI_EQUIV_SKIP=1) without it.
fn have_checkupdates() -> bool {
    crate::testenv::require("checkupdates") && crate::testenv::require("bash")
}

/// Every program in /usr/bin except `but`, as links in one directory: a PATH on which the
/// script finds its tools but not those.
fn sysbin(dir: &Path, but: &[&str]) -> PathBuf {
    let d = dir.join("sysbin");
    std::fs::create_dir_all(&d).unwrap();
    for e in std::fs::read_dir("/usr/bin").unwrap().flatten() {
        let n = e.file_name();
        if !but.iter().any(|b| n == *b) {
            let _ = std::os::unix::fs::symlink(e.path(), d.join(&n));
        }
    }
    d
}

struct Case {
    what: &'static str,
    /// `pacman -Sy` and `pacman -Qu` stub bodies.
    sync: &'static str,
    query: &'static str,
    /// `pacman-conf DBPath` stub body (`$DBDIR` is an existing directory); `None`: absent.
    conf: Option<&'static str>,
    fakeroot: bool,
    /// Run with CHECKUPDATES_DB set (else the `$TMPDIR/checkup-db-$UID/` default).
    db_env: bool,
    /// A stale lock left in the database directory.
    stale_lock: bool,
}

const C: Case = Case {
    what: "",
    sync: "exit 0",
    query: "printf 'linux 1 -> 2\\nmesa 1 -> 2\\n'",
    conf: Some("echo \"$DBDIR\""),
    fakeroot: true,
    db_env: true,
    stale_lock: false,
};

/// One side of a case in its own directory: stdout, stderr, exit code, the commands the
/// stubs saw, where the database's `local` link points, and whether the lock is left.
fn side(
    c: &Case,
    root: &Path,
    native: bool,
) -> ((String, String, i32), String, Option<PathBuf>, bool) {
    let stubs = root.join("stubs");
    std::fs::create_dir_all(&stubs).unwrap();
    let log = root.join("log");
    let dbdir = root.join("pacmandb");
    std::fs::create_dir_all(&dbdir).unwrap();
    let pre = format!("DBDIR='{}'; LOG='{}'", dbdir.display(), log.display());
    stub(
        &stubs,
        "pacman",
        &format!(
            "{pre}; echo \"pacman $*\" >> \"$LOG\"\ncase \"$1\" in -Sy) {};; -Qu) {};; esac",
            c.sync, c.query
        ),
        true,
    );
    if let Some(conf) = c.conf {
        stub(
            &stubs,
            "pacman-conf",
            &format!("{pre}; echo \"pacman-conf $*\" >> \"$LOG\"; {conf}"),
            true,
        );
    }
    if c.fakeroot {
        stub(
            &stubs,
            "fakeroot",
            &format!(
                "{pre}; echo \"fakeroot $*\" >> \"$LOG\"; [ \"$1\" = -- ] && shift; exec \"$@\""
            ),
            true,
        );
    }
    let sys = sysbin(root, &["pacman", "pacman-conf", "fakeroot"]);
    let path: OsString = format!("{}:{}", stubs.display(), sys.display()).into();
    let tmpdir = root.join("tmp");
    std::fs::create_dir_all(&tmpdir).unwrap();
    let db = if c.db_env {
        root.join("db")
    } else {
        // SAFETY: getuid(2) takes no arguments and cannot fail.
        tmpdir.join(format!("checkup-db-{}", unsafe { libc::getuid() }))
    };
    if c.stale_lock {
        std::fs::create_dir_all(&db).unwrap();
        std::fs::write(db.join("db.lck"), "").unwrap();
    }
    let mut env: HashMap<&str, OsString> = HashMap::new();
    env.insert("TMPDIR", tmpdir.into());
    if c.db_env {
        env.insert("CHECKUPDATES_DB", db.clone().into());
    }
    let o = if native {
        let stop: Arc<Stop> = Default::default();
        count_updates(&path, &|k| env.get(k).cloned(), &stop, &Default::default())
    } else {
        real_env(COUNT_UPDATES, &path, &env)
    };
    let seen = std::fs::read_to_string(&log)
        .unwrap_or_default()
        .replace(&root.display().to_string(), "ROOT");
    let link = std::fs::read_link(db.join("local")).ok().map(|l| {
        PathBuf::from(
            l.to_string_lossy()
                .replace(&root.display().to_string(), "ROOT"),
        )
    });
    (text(&o), seen, link, db.join("db.lck").exists())
}

#[test]
fn count_updates_same_as_checkupdates() {
    if !have_checkupdates() {
        return;
    }
    let cases = [
        Case {
            what: "updates",
            ..C
        },
        Case {
            what: "ignored ones dropped",
            query: "printf 'linux 1 -> 2\\nfoo 1 -> 2 [ignored]\\nbar 1 -> 2 [игнорируется]\\n'",
            ..C
        },
        Case {
            what: "only ignored",
            query: "echo 'foo 1 -> 2 [ignored]'",
            ..C
        },
        Case {
            what: "none",
            query: "exit 1",
            ..C
        },
        Case {
            what: "no trailing newline",
            query: "printf 'a 1 -> 2\\nb 1 -> 2'",
            ..C
        },
        Case {
            what: "blank line",
            query: "printf 'a 1 -> 2\\n\\nb 1 -> 2\\n'",
            ..C
        },
        Case {
            what: "query stderr",
            query: "echo 'a 1 -> 2'; echo warn >&2",
            ..C
        },
        Case {
            what: "sync fails",
            sync: "exit 1",
            ..C
        },
        Case {
            what: "sync chatter",
            sync: "echo syncing; echo warning >&2",
            ..C
        },
        Case {
            what: "no fakeroot",
            fakeroot: false,
            ..C
        },
        Case {
            what: "DBPath empty",
            conf: Some("true"),
            ..C
        },
        Case {
            what: "DBPath not a directory",
            conf: Some("echo /nonexistent/db"),
            ..C
        },
        Case {
            what: "pacman-conf stderr",
            conf: Some("echo \"$DBDIR\"; echo note >&2"),
            ..C
        },
        Case {
            what: "TMPDIR default database",
            db_env: false,
            ..C
        },
        Case {
            what: "stale lock removed",
            stale_lock: true,
            ..C
        },
    ];
    for c in &cases {
        let root = tmp("cu");
        let (a, b) = (root.join("a"), root.join("b"));
        // Same directory name on both sides, so the logged paths compare equal.
        let (ra, rb) = (a.join("r"), b.join("r"));
        let r = side(c, &ra, false);
        let o = side(c, &rb, true);
        assert_eq!(o, r, "{}", c.what);
        let _ = std::fs::remove_dir_all(&root);
    }
    // Without pacman-conf the script's shell prints its own `command not found`; the rest
    // is the same.
    let root = tmp("cu-noconf");
    let c = Case {
        what: "no pacman-conf",
        conf: None,
        ..C
    };
    let (r, o) = (
        side(&c, &root.join("a/r"), false),
        side(&c, &root.join("b/r"), true),
    );
    assert_eq!(
        (&o.0.0, o.0.2, &o.1, &o.2, o.3),
        (&r.0.0, r.0.2, &r.1, &r.2, r.3)
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// The real pacman (through `--config`, on a private `file://` repository and local
/// database): what pacman itself counts as an upgrade, ignored and epoch versions included.
#[test]
fn count_updates_with_real_pacman_same_as_checkupdates() {
    if !have_checkupdates()
        || !crate::testenv::require("pacman")
        || !crate::testenv::require("pacman-conf")
        || !crate::testenv::require("fakeroot")
        || !crate::testenv::require("tar")
    {
        return;
    }
    let t = tmp("cu-real");
    let desc = |dir: &Path, name: &str, ver: &str| {
        // pacman takes the version from the entry name too: `ep-1:0.5-1`.
        let d = dir.join(format!("{name}-{ver}"));
        std::fs::create_dir_all(&d).unwrap();
        let body = format!("%NAME%\n{name}\n\n%VERSION%\n{ver}\n\n%ARCH%\nany\n\n");
        std::fs::write(d.join("desc"), body).unwrap();
        d.file_name().unwrap().to_owned()
    };
    let local = t.join("db/local");
    std::fs::create_dir_all(&local).unwrap();
    std::fs::write(local.join("ALPM_DB_VERSION"), "9\n").unwrap();
    for (n, v) in [
        ("foo", "1.0-1"),
        ("bar", "1.0-1"),
        ("baz", "3-1"),
        ("qux", "2.0-1"),
        ("ep", "1:0.5-1"),
        ("zed", "1.0-1"),
        ("gone", "1-1"),
    ] {
        desc(&local, n, v);
    }
    let src = t.join("src");
    std::fs::create_dir_all(&src).unwrap();
    let mut names = vec![];
    for (n, v) in [
        ("foo", "1.1-1"),
        ("bar", "1.1-1"),
        ("baz", "3-1"),
        ("qux", "1.0-1"),
        ("ep", "2.0-1"),
        ("zed", "1.0-2"),
    ] {
        names.push(desc(&src, n, v));
    }
    std::fs::create_dir_all(t.join("repo")).unwrap();
    let st = Command::new("tar")
        .arg("-czf")
        .arg(t.join("repo/test.db"))
        .arg("-C")
        .arg(&src)
        .args(&names)
        .status()
        .unwrap();
    assert!(st.success());
    let conf = t.join("pacman.conf");
    std::fs::write(
        &conf,
        format!(
            "[options]\nDBPath = {}/db/\nArchitecture = auto\nSigLevel = Never\nIgnorePkg = bar\n\n[test]\nServer = file://{}/repo\n",
            t.display(),
            t.display()
        ),
    )
    .unwrap();
    let bin = t.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    for p in ["pacman", "pacman-conf"] {
        stub(
            &bin,
            p,
            &format!("exec /usr/bin/{p} --config '{}' \"$@\"", conf.display()),
            true,
        );
    }
    let path = path_with(&bin);
    let last = Default::default();
    let run = |side: &str, native: bool| {
        let mut env: HashMap<&str, OsString> = HashMap::new();
        env.insert("CHECKUPDATES_DB", t.join(side).into());
        if native {
            count_updates(&path, &|k| env.get(k).cloned(), &Default::default(), &last)
        } else {
            real_env(COUNT_UPDATES, &path, &env)
        }
    };
    let r = run("a", false);
    assert_eq!(
        text(&r),
        ("2\n".into(), String::new(), 0),
        "foo and zed, bar ignored"
    );
    assert_eq!(text(&run("b", true)), text(&r));
    std::fs::remove_file(t.join("repo/test.db")).unwrap();
    // Unreachable repository: the script's pipeline says 0; the native check keeps the
    // count it last got, with the same error line.
    let r = run("c", false);
    assert_eq!(r.out, b"0\n");
    let o = run("d", true);
    assert_eq!(text(&o), ("2\n".into(), text(&r).1, 0));
    let _ = std::fs::remove_dir_all(&t);
}

#[test]
fn which_same_as_which() {
    let d = tmp("wh");
    stub(&d, "checkupdates", "true", true);
    let n = tmp("wh-noexec");
    stub(&n, "checkupdates", "true", false);
    for (what, path) in [
        ("present", path_with(&d)),
        ("not executable", path_with(&n)),
        ("missing", path_with(&tmp("wh-missing"))),
    ] {
        assert_eq!(
            installed("checkupdates", &path),
            real(WHICH_CHECKUPDATES, &path).code == 0,
            "{what}"
        );
    }
}

/// A check that cannot run says why on stderr, in its own words.
#[test]
fn a_failed_check_says_why() {
    for (c, why) in [
        (
            Case {
                what: "sync fails",
                sync: "exit 1",
                ..C
            },
            "updates: cannot sync the package databases\n",
        ),
        (
            Case {
                what: "no fakeroot",
                fakeroot: false,
                ..C
            },
            "updates: no fakeroot in PATH\n",
        ),
    ] {
        let root = tmp("cu-why");
        let stop: Arc<Stop> = Default::default();
        let stubs = side(&c, &root.join("r"), true);
        assert_eq!((stubs.0.0.as_str(), stubs.0.2), ("0\n", 0), "{}", c.what);
        let path: OsString = format!(
            "{}:{}",
            root.join("r/stubs").display(),
            root.join("r/sysbin").display()
        )
        .into();
        let env = |k: &str| (k == "CHECKUPDATES_DB").then(|| root.join("r/db").into());
        let o = checkupdates(&path, &env, &stop).unwrap();
        assert_eq!(
            (String::from_utf8_lossy(&o.err).as_ref(), o.code),
            (why, 1),
            "{}",
            c.what
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}

/// A stopped check returns at once and ends the sync's whole process group (pacman's own
/// children included), and drops the lock as checkupdates' TERM trap does.
#[test]
fn stop_ends_the_sync_group() {
    let d = tmp("stop-cu");
    let pid = d.join("pid");
    fifo(&pid);
    let db = d.join("db");
    stub(
        &d,
        "fakeroot",
        "[ \"$1\" = -- ] && shift; exec \"$@\"",
        true,
    );
    stub(&d, "pacman-conf", "true", true);
    stub(
        &d,
        "pacman",
        &format!(
            "case \"$1\" in -Sy) : > '{}/db.lck'; sleep infinity & echo $! > '{}'; wait;; esac",
            db.display(),
            pid.display()
        ),
        true,
    );
    let stop: Arc<Stop> = Default::default();
    let s = stop.clone();
    let (path, dbv) = (path_with(&d), OsString::from(&db));
    let h = std::thread::spawn(move || {
        count_updates(
            &path,
            &|k| (k == "CHECKUPDATES_DB").then(|| dbv.clone()),
            &s,
            &Default::default(),
        )
    });
    // pacman's child never exits on its own: the stop must end it.
    let child: i32 = std::fs::read_to_string(&pid)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    // Opened while the child runs (its parent's `wait` keeps it from being reaped).
    let child = workpool::pidfd_open(child).unwrap();
    stop.store(true, Ordering::Relaxed);
    let r = h.join().unwrap();
    assert_eq!(r.code, -1);
    assert!(!db.join("db.lck").exists(), "lock left behind");
    // Readable once pacman's child terminated (pidfd_open(2)).
    let mut p = libc::pollfd {
        fd: std::os::fd::AsRawFd::as_raw_fd(&child),
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: poll(2) reads and writes only the one pollfd it is given.
    while unsafe { libc::poll(&mut p, 1, -1) } != 1 {
        let e = std::io::Error::last_os_error();
        assert_eq!(e.kind(), std::io::ErrorKind::Interrupted, "poll: {e}");
    }
}
