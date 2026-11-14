//! The Updates service's two questions, answered without a shell: whether `checkupdates` is
//! installed (a PATH search), and how many updates are pending (what pacman-contrib's
//! `checkupdates` does: the `pacman` calls in a private sync database, counted here). A stop
//! ends each child's whole process group.
use crate::jobs::{Job, Pipes};
use crate::svc::Stop;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::Ordering;

/// `UpdatesProbe`: exits 0 when `checkupdates` is installed, 1 when it is not.
pub fn probe_job() -> Job {
    let path = user_path();
    Box::new(move |_: &Pipes, _: &Arc<Stop>| {
        if installed("checkupdates", &path) {
            0
        } else {
            1
        }
    })
}

/// `UpdatesCount`: the number of pending updates on stdout ([`count_updates`]).
pub fn count_job() -> Job {
    let path = user_path();
    Box::new(move |io: &Pipes, stop: &Arc<Stop>| {
        pipe(
            io,
            count_updates(&path, &|k| workpool::userenv::var(k), stop, &LAST_COUNT),
        )
    })
}

/// PATH as the commands the original ran saw it: the user's, not the shell's own after its pragmas.
fn user_path() -> OsString {
    workpool::userenv::var("PATH").unwrap_or_default()
}

fn pipe(io: &Pipes, r: Out) -> i32 {
    if !r.out.is_empty() {
        io.out(String::from_utf8_lossy(&r.out));
    }
    if !r.err.is_empty() {
        io.err(String::from_utf8_lossy(&r.err));
    }
    r.code
}

#[derive(Debug, Default, PartialEq)]
pub struct Out {
    pub out: Vec<u8>,
    pub err: Vec<u8>,
    pub code: i32,
}

/// Whether `path` (a PATH) has an executable `name`.
pub fn installed(name: &str, path: &OsStr) -> bool {
    crate::osutil::which(name, path).is_some()
}

/// `prog args…` with `path` as PATH: stdout, stderr and exit code (127 when there is no
/// such program, 126 when it cannot be executed). `None` when `stop` ended it.
pub fn run(prog: &str, args: &[&str], path: &OsStr, stop: &Arc<Stop>) -> Option<Out> {
    // Looked up in `path`, not the user's PATH: the name stays for `execvp`.
    let mut c = std::process::Command::new(prog);
    c.args(args).env("PATH", path).stdin(Stdio::null());
    workpool::userenv::apply(&mut c);
    match crate::child::run(&mut c, stop) {
        Ok(o) => Some(Out {
            out: o.out,
            err: o.err,
            code: o.status?.code().unwrap_or(-1),
        }),
        Err(e) => Some(Out {
            code: if e.kind() == std::io::ErrorKind::NotFound {
                127
            } else {
                126
            },
            ..Default::default()
        }),
    }
}

/// The process environment, as a lookup (tests pass their own).
pub type Env<'a> = &'a dyn Fn(&str) -> Option<OsString>;

fn non_empty(env: Env, k: &str) -> Option<OsString> {
    env(k).filter(|v| !v.is_empty())
}

/// pacman-contrib's `checkupdates` (no options): sync a private copy of the databases with
/// `fakeroot -- pacman -Sy --dbpath DB`, list `pacman -Qu --dbpath DB` minus ignored
/// (bracketed) lines. Code 0 with updates, 2 with none, 1 (and the reason on stderr) when
/// it cannot sync. `None` when `stop` ended it.
pub fn checkupdates(path: &OsStr, env: Env, stop: &Arc<Stop>) -> Option<Out> {
    let die = |msg: &str, err: Vec<u8>| {
        let mut err = err;
        err.extend_from_slice(format!("updates: {msg}\n").as_bytes());
        Some(Out {
            err,
            code: 1,
            ..Default::default()
        })
    };
    if !crate::osutil::candidates("fakeroot", path).any(|p| crate::osutil::access_x(&p)) {
        return die("no fakeroot in PATH", vec![]);
    }
    let db: OsString = non_empty(env, "CHECKUPDATES_DB").unwrap_or_else(|| {
        let mut d = non_empty(env, "TMPDIR").unwrap_or_else(|| "/tmp".into());
        // SAFETY: getuid(2) takes no arguments and cannot fail.
        d.push(format!("/checkup-db-{}/", unsafe { libc::getuid() }));
        d
    });
    let mut lock = db.clone();
    lock.push("/db.lck");
    // The lock goes however the check ends, as checkupdates' exit trap removes it.
    struct Unlock(OsString);
    impl Drop for Unlock {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _unlock = Unlock(lock);
    let conf = run("pacman-conf", &["DBPath"], path, stop)?;
    let err = conf.err;
    let mut dbpath = conf.out;
    while dbpath.last() == Some(&b'\n') {
        dbpath.pop();
    }
    let mut dbpath = OsString::from_vec(dbpath);
    if dbpath.is_empty() || !std::path::Path::new(&dbpath).is_dir() {
        dbpath = "/var/lib/pacman/".into();
    }
    let _ = std::fs::create_dir_all(&db);
    dbpath.push("/local");
    let _ = std::os::unix::fs::symlink(&dbpath, std::path::Path::new(&db).join("local"));
    let db = db
        .to_str()
        .map(str::to_owned)
        .unwrap_or_else(|| db.to_string_lossy().into());
    let sync = [
        "--",
        "pacman",
        "-Sy",
        "--disable-sandbox-filesystem",
        "--dbpath",
        &db,
        "--logfile",
        "/dev/null",
    ];
    if run("fakeroot", &sync, path, stop)?.code != 0 {
        return die("cannot sync the package databases", err);
    }
    let q = run("pacman", &["-Qu", "--dbpath", &db], path, stop)?;
    let mut lines: Vec<&[u8]> = q.out.split(|&b| b == b'\n').collect();
    if q.out.is_empty() || q.out.ends_with(b"\n") {
        lines.pop();
    }
    let mut out = vec![];
    for l in lines.into_iter().filter(|l| !ignored(l)) {
        out.extend_from_slice(l);
        out.push(b'\n');
    }
    let code = if out.is_empty() { 2 } else { 0 };
    Some(Out { out, err, code })
}

/// Whether pacman marked the upgrade ignored: a bracketed word (`[ignored]`, translated).
fn ignored(l: &[u8]) -> bool {
    l.iter()
        .position(|&b| b == b'[')
        .is_some_and(|i| l[i + 1..].contains(&b']'))
}

/// The last count a check got, for the checks that fail after it.
static LAST_COUNT: std::sync::Mutex<Option<usize>> = std::sync::Mutex::new(None);

/// The number of pending updates as a line on stdout, code 0; what the check wrote on stderr
/// passes through. A check that cannot sync (offline, mirror down) reports the `last` count
/// it knows instead of 0, so a transient failure does not clear the bar's update count.
/// Code -1 and nothing when `stop` ended it.
pub fn count_updates(
    path: &OsStr,
    env: Env,
    stop: &Arc<Stop>,
    last: &std::sync::Mutex<Option<usize>>,
) -> Out {
    match checkupdates(path, env, stop) {
        Some(r) if !stop.load(Ordering::Relaxed) => {
            let mut last = last.lock().unwrap_or_else(|e| e.into_inner());
            let n = if r.code == 1 {
                last.unwrap_or(0)
            } else {
                let n = r.out.iter().filter(|&&b| b == b'\n').count();
                *last = Some(n);
                n
            };
            Out {
                out: format!("{n}\n").into_bytes(),
                err: r.err,
                code: 0,
            }
        }
        _ => Out {
            code: -1,
            ..Default::default()
        },
    }
}

#[cfg(test)]
mod tests;
