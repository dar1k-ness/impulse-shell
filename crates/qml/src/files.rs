//! `Impulse.Files`: the file commands the original ran through `bash -c`, `rm`, `mkdir`, `sed` and
//! `ls` (the first-run marker of FirstRunExperience.qml and welcome.qml, the startup cleanup
//! of Directories.qml, MessageCodeBlock's "save code", GeneralConfig's hyprlock time format,
//! SessionWarnings' lock and partial-download files). Each call runs on a pool thread, as
//! each command was its own detached process; `writeLine` and `list` answer with the
//! `written` and `listed` signals.
//!
//! Under a spawn hook (aotcheck, tests) the call (`Files`, the method, its arguments) goes to
//! the hook and the disk is left alone; `writeLine` then answers without an error and `list`
//! with no names.
//!
//! What the calls do that the commands did not:
//! - a path or text holding a `'` broke the original's `'...'` quoting so bash ran nothing, and
//!   `echo 'TEXT'` swallowed a TEXT of only `-n`/`-e`/`-E` flags; here nothing is quoted;
//! - a missing directory made a write fail; `writeLine` creates it and reports a failure;
//! - `sed -i 's/\TIME12\b/TIME/'` changed only the first match on a line, any `TIME12`
//!   word, and replaced a symlinked hyprlock.conf with a copy; `renameVariable` switches
//!   every `$TIME12` variable, in the file the link points to, rewritten only when it changes;
//! - `ls DIR | grep` missed hidden files and mangled names with newlines; `list` gives every
//!   entry of the directory.

use crate::eval::{Interp, R};
use crate::runtime::World;
use crate::value::Value;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

static RUNNING: AtomicUsize = AtomicUsize::new(0);

/// Calls of this module still running (tests wait for 0).
pub fn running() -> usize {
    RUNNING.load(Ordering::SeqCst)
}

thread_local!(static LIVE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });

/// Run this module's calls on this thread even under a spawn hook, which keeps sandboxing
/// the rest of the original (tests).
pub fn set_live(on: bool) {
    LIVE.set(on);
}

pub(crate) fn hooked() -> bool {
    crate::native::sandboxed() && !LIVE.get()
}

/// Run `op`, the work of `method` called with `args`, on its own thread, or under a spawn
/// hook only hand the call to the hook; whether `op` was started.
fn detached(method: &str, args: &[&str], op: impl FnOnce() + Send + 'static) -> bool {
    if hooked() {
        crate::native::hooked_call(TYPE.name, method, args);
        return false;
    }
    RUNNING.fetch_add(1, Ordering::SeqCst);
    workpool::run(move || {
        op();
        RUNNING.fetch_sub(1, Ordering::SeqCst);
    });
    true
}

fn arg(args: &[Value], i: usize) -> String {
    args.get(i).map(Value::to_js_string).unwrap_or_default()
}

/// The way back to this world's main thread and a new call id.
fn ticket(it: &mut Interp) -> Option<(crate::impulse::Replies, i32)> {
    let w = crate::runtime::world_of(it)?;
    let mut w = w.borrow_mut();
    let back = crate::impulse::replies(&mut w);
    Some((back, crate::processes::next_id(&mut w, TYPE.name)))
}

fn written(w: &mut World, id: i32, error: &str) {
    if let Some(&item) = w.singletons.get("Files") {
        crate::native::emit(
            w,
            item,
            "written",
            &["id", "error"],
            vec![Value::Num(id as f64), Value::str(error)],
        );
    }
}

fn listed(w: &mut World, id: i32, names: &[String]) {
    if let Some(&item) = w.singletons.get("Files") {
        let names = names.iter().map(|n| Value::str(n)).collect();
        crate::native::emit(
            w,
            item,
            "listed",
            &["id", "names"],
            vec![Value::Num(id as f64), Value::array(names)],
        );
    }
}

// ---- the methods ----

pub static TYPE: crate::impulse::Type = crate::impulse::Type {
    name: "Files",
    methods: METHODS,
    init: None,
};

const METHODS: &[(&str, crate::eval::NativeFn)] = &[
    ("remove", remove),
    ("removeTree", remove_tree_call),
    ("makeDir", make_dir),
    ("renewDir", renew_dir),
    ("renewPrivateDir", renew_private_dir),
    ("writeLine", write_line_call),
    ("renameVariable", rename_variable),
    ("list", list),
];

/// `void remove(string path)`: unlink PATH unless it is a directory (`rm -f`).
pub fn remove(_: &mut Interp, _this: &Value, args: &[Value]) -> R<Value> {
    let p = arg(args, 0);
    detached("remove", &[&p.clone()], move || rm_file(&p));
    Ok(Value::Undefined)
}

/// `void removeTree(string path)`: PATH and everything in it (`rm -rf`), reached without
/// following a symlink on the way (the original's directories in the shared `/tmp/impulse-shell`).
pub fn remove_tree_call(_: &mut Interp, _this: &Value, args: &[Value]) -> R<Value> {
    let p = arg(args, 0);
    detached("removeTree", &[&p.clone()], move || {
        if let Ok((dir, name)) = crate::privfs::Dir::parent_of(p.as_ref(), false) {
            dir.remove_all(name);
        }
    });
    Ok(Value::Undefined)
}

/// `void makeDir(string path)`: PATH and its missing parents (`mkdir -p`).
pub fn make_dir(_: &mut Interp, _this: &Value, args: &[Value]) -> R<Value> {
    let p = arg(args, 0);
    detached("makeDir", &[&p.clone()], move || mkdir_p(&p));
    Ok(Value::Undefined)
}

/// `void renewDir(string path)`: PATH emptied and there (`rm -rf PATH; mkdir -p PATH`).
pub fn renew_dir(_: &mut Interp, _this: &Value, args: &[Value]) -> R<Value> {
    let p = arg(args, 0);
    detached("renewDir", &[&p.clone()], move || {
        rm_rf(&p);
        mkdir_p(&p);
    });
    Ok(Value::Undefined)
}

/// `void renewPrivateDir(string path)`: [`renew_dir`] for a directory only this user may
/// enter, made without following a symlink on the way (decoded clipboard images in `/tmp`).
pub fn renew_private_dir(_: &mut Interp, _this: &Value, args: &[Value]) -> R<Value> {
    let p = arg(args, 0);
    detached("renewPrivateDir", &[&p.clone()], move || {
        if let Err(e) = crate::privfs::Dir::renew(p.as_ref()) {
            elog::eprintln!("[Files] {p}: {e}");
        }
    });
    Ok(Value::Undefined)
}

/// `int writeLine(string path, string text)`: TEXT and a newline into PATH; the id
/// `written(id, error)` will carry, with an empty `error` when the file was written.
pub fn write_line_call(it: &mut Interp, _this: &Value, args: &[Value]) -> R<Value> {
    let (p, text) = (arg(args, 0), arg(args, 1));
    let Some((back, id)) = ticket(it) else {
        return Ok(Value::Num(-1.0));
    };
    let done = back.clone();
    let started = detached("writeLine", &[&p.clone(), &text.clone()], move || {
        let error = write_line(&p, &text).err().map(|e| os_msg(&e));
        let error = error.unwrap_or_default();
        done.send(move |w| written(w, id, &error));
    });
    if !started {
        back.send(move |w| written(w, id, ""));
    }
    Ok(Value::Num(id as f64))
}

/// `void renameVariable(string path, string from, string to)`: every `$FROM` variable of
/// the hyprlock config PATH becomes `$TO`.
pub fn rename_variable(_: &mut Interp, _this: &Value, args: &[Value]) -> R<Value> {
    let (p, from, to) = (arg(args, 0), arg(args, 1), arg(args, 2));
    if from.is_empty() {
        return Ok(Value::Undefined);
    }
    let args = [&*p.clone(), &*from.clone(), &*to.clone()];
    detached("renameVariable", &args, move || {
        swap_time_var(&p, &from, &to)
    });
    Ok(Value::Undefined)
}

/// `int list(string dir)`: the id `listed(id, names)` will carry, with the name of every
/// entry of DIR (none when it cannot be read).
pub fn list(it: &mut Interp, _this: &Value, args: &[Value]) -> R<Value> {
    let d = arg(args, 0);
    let Some((back, id)) = ticket(it) else {
        return Ok(Value::Num(-1.0));
    };
    let done = back.clone();
    let started = detached("list", &[&d.clone()], move || {
        let names = entries(&d);
        done.send(move |w| listed(w, id, &names));
    });
    if !started {
        back.send(move |w| listed(w, id, &[]));
    }
    Ok(Value::Num(id as f64))
}

// ---- file operations ----

/// TEXT and a newline into FILE (`0666 & ~umask`), its missing directories created first.
fn write_line(file: &str, text: &str) -> std::io::Result<()> {
    if let Some(d) = Path::new(file).parent().and_then(Path::to_str) {
        mkdir_p(d);
    }
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o666)
        .open(file)?;
    f.write_all(text.as_bytes())?;
    f.write_all(b"\n")
}

/// `rm [-f] FILE`: unlinks anything but a directory (stdin is not a terminal, so `rm`
/// without `-f` never prompts).
fn rm_file(path: &str) {
    if let Ok(m) = std::fs::symlink_metadata(path)
        && !m.is_dir()
    {
        let _ = std::fs::remove_file(path);
    }
}

/// `rm -rf PATH`: removes what it can and goes on past errors; symlinks are removed, never
/// followed; `/`, `.` and `..` are refused.
fn rm_rf(path: &str) {
    let p = Path::new(path);
    let refused = path.is_empty()
        || p.parent().is_none()
        || matches!(
            path.rsplit('/').find(|c| !c.is_empty()),
            Some(".") | Some("..")
        );
    if !refused {
        remove_tree(p);
    }
}

fn remove_tree(p: &Path) {
    let Ok(m) = std::fs::symlink_metadata(p) else {
        return;
    };
    if !m.is_dir() {
        let _ = std::fs::remove_file(p);
        return;
    }
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            remove_tree(&e.path());
        }
    }
    let _ = std::fs::remove_dir(p);
}

/// The process umask as the kernel reports it (`/proc/self/status`), read without the
/// `umask(2)` swap, which would race other threads creating files.
fn proc_umask() -> Option<u32> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    let l = s.lines().find_map(|l| l.strip_prefix("Umask:"))?;
    u32::from_str_radix(l.trim(), 8).ok()
}

/// `mkdir -p PATH` (coreutils): missing parents get `0777 & ~umask` plus `u+wx`, the last
/// component `0777 & ~umask`. The kernel applies the umask; it is never read or changed here
/// (the process mask is shared by every thread).
fn mkdir_p(path: &str) {
    if path.is_empty() {
        return;
    }
    let p = Path::new(path);
    if proc_umask().is_some_and(|m| m & 0o300 == 0) {
        // `u+wx` is already in the mask's mode: every component gets `0777 & ~umask`.
        let _ = std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o777)
            .create(p);
        return;
    }
    // A umask without `u+wx` leaves new parents unenterable: each needs its `u+wx` before
    // the next one can be made inside it.
    if p.is_dir() {
        return;
    }
    let mut todo = vec![];
    let mut cur = Some(p);
    while let Some(c) = cur.filter(|c| !c.as_os_str().is_empty() && !c.is_dir()) {
        todo.push(c);
        cur = c.parent();
    }
    // A parent made under a umask without u+wx gets them only from the chmod below: no other
    // job thread may mkdir into it before that (EACCES for non-root).
    static CREATE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g = CREATE.lock().unwrap_or_else(|e| e.into_inner());
    let n = todo.len();
    for (i, d) in todo.into_iter().rev().enumerate() {
        let last = i + 1 == n;
        match std::fs::DirBuilder::new().mode(0o777).create(d) {
            Ok(()) if !last => {
                if let Ok(m) = std::fs::symlink_metadata(d)
                    && m.mode() & 0o300 != 0o300
                {
                    let mode = (m.mode() & 0o7777) | 0o300;
                    let _ = std::fs::set_permissions(d, std::fs::Permissions::from_mode(mode));
                }
            }
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && d.is_dir() => {}
            Err(_) => return,
        }
    }
}

/// glibc regex `\b` after a word character: the next character is not alphanumeric or `_`
/// (multibyte-aware, as in a UTF-8 locale; an invalid byte is not a word character).
fn word_ends(rest: &[u8]) -> bool {
    let Some(&b) = rest.first() else {
        return true;
    };
    if b < 0x80 {
        return !(b.is_ascii_alphanumeric() || b == b'_');
    }
    let n = rest.len().min(4);
    let c = (1..=n).find_map(|k| std::str::from_utf8(&rest[..k]).ok()?.chars().next());
    !c.is_some_and(char::is_alphanumeric)
}

/// Every hyprlock variable `$FROM` (ending a word) in LINE as `$TO`; whether any was.
fn swap_var(line: &[u8], from: &[u8], to: &[u8], out: &mut Vec<u8>) -> bool {
    let mut hit = false;
    let mut i = 0;
    while i < line.len() {
        let rest = &line[i + 1..];
        if line[i] == b'$' && rest.starts_with(from) && word_ends(&rest[from.len()..]) {
            out.push(b'$');
            out.extend_from_slice(to);
            i += 1 + from.len();
            hit = true;
        } else {
            out.push(line[i]);
            i += 1;
        }
    }
    hit
}

/// `$FROM` -> `$TO` in FILE, edited where a symlink points: a temp file beside it with its
/// mode (and owner, when allowed) renamed over it, only when something changed.
fn swap_time_var(file: &str, from: &str, to: &str) {
    let Ok(p) = std::fs::canonicalize(file) else {
        return;
    };
    let Ok(m) = std::fs::metadata(&p) else {
        return;
    };
    let (true, Ok(data)) = (m.is_file(), std::fs::read(&p)) else {
        return;
    };
    let mut out = Vec::with_capacity(data.len() + 8);
    if !swap_var(&data, from.as_bytes(), to.as_bytes(), &mut out) {
        return;
    }
    let Some(dir) = p.parent() else {
        return;
    };
    let Ok(mut tmp) = tempfile::Builder::new()
        .prefix(".hyprlock")
        .permissions(std::fs::Permissions::from_mode(0o600))
        .tempfile_in(dir)
    else {
        return;
    };
    let f = tmp.as_file_mut();
    let _ = std::os::unix::fs::fchown(&*f, Some(m.uid()), Some(m.gid()));
    let _ = f.set_permissions(std::fs::Permissions::from_mode(m.mode() & 0o7777));
    if f.write_all(&out).is_err() || f.sync_all().is_err() {
        return;
    }
    let _ = tmp.persist(&p);
}

/// The name of every entry of DIR, hidden ones too.
fn entries(dir: &str) -> Vec<String> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect()
}

/// strerror text without Rust's " (os error N)" suffix.
fn os_msg(e: &std::io::Error) -> String {
    let s = e.to_string();
    match s.find(" (os error") {
        Some(i) => s[..i].to_string(),
        None => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_var_swap() {
        let s = |l: &str, f: &str, t: &str| {
            let mut o = vec![];
            let hit = swap_var(l.as_bytes(), f.as_bytes(), t.as_bytes(), &mut o);
            (String::from_utf8(o).unwrap(), hit)
        };
        let ok = |o: &str| (o.to_string(), true);
        assert_eq!(s("$TIME12 $TIME12", "TIME12", "TIME"), ok("$TIME $TIME"));
        assert!(!s("TIME12 x$TIME12a", "TIME12", "TIME").1);
        assert!(!s("$TIME12", "TIME", "TIME12").1);
        assert_eq!(s("$TIMEé $TIME", "TIME", "TIME12"), ok("$TIMEé $TIME12"));
        assert_eq!(s("$TIME_ $TIME-", "TIME", "TIME12"), ok("$TIME_ $TIME12-"));
        assert_eq!(s("a $TIME\n$", "TIME", "TIME12"), ok("a $TIME12\n$"));
    }

    // ---- umask (from integ) ----

    fn tree_modes(root: &Path) -> Vec<(String, u32)> {
        let mut v = vec![];
        let mut d = root.to_path_buf();
        for name in ["a", "b", "c"] {
            d.push(name);
            let m = std::fs::symlink_metadata(&d).map(|m| m.mode() & 0o7777);
            v.push((name.into(), m.unwrap_or(0)));
        }
        v
    }

    fn proc_umask() -> String {
        std::fs::read_to_string("/proc/self/status")
            .unwrap()
            .lines()
            .find_map(|l| l.strip_prefix("Umask:").map(|m| m.trim().to_string()))
            .unwrap()
    }

    /// `mkdir -p` (coreutils) and the native one leave the same modes under each umask, and the
    /// native one never changes the process mask. Runs in a re-exec'd copy of this test binary
    /// so the umask it sets cannot reach other tests' threads.
    #[test]
    fn mkdir_p_matches_coreutils_under_umask() {
        if let Ok(mask) = std::env::var("RI_MKDIR_UMASK_CHILD") {
            let mask = u32::from_str_radix(&mask, 8).unwrap();
            if !crate::testenv::require("mkdir") {
                return;
            }
            let t = tempfile::tempdir().unwrap();
            let (r, n) = (t.path().join("ref"), t.path().join("native"));
            // SAFETY: this child process runs only this test.
            let saved = unsafe { libc::umask(mask as libc::mode_t) };
            let st = std::process::Command::new("mkdir")
                .arg("-p")
                .arg(r.join("a/b/c"))
                .status()
                .unwrap();
            let before = proc_umask();
            mkdir_p(n.join("a/b/c").to_str().unwrap());
            let after = proc_umask();
            let first = tree_modes(&n);
            mkdir_p(n.join("a/b/c").to_str().unwrap());
            let again = tree_modes(&n);
            // Files this process writes at exit (coverage profiles) must stay readable.
            // SAFETY: as above.
            unsafe { libc::umask(saved) };
            assert!(st.success());
            assert_eq!(after, before);
            let want = tree_modes(&r);
            assert_eq!(first, want, "umask {mask:o}");
            assert_eq!(again, want, "umask {mask:o}, again");
            return;
        }
        for mask in ["022", "077", "027", "777", "000"] {
            let o = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "files::tests::mkdir_p_matches_coreutils_under_umask",
                    "--test-threads=1",
                ])
                .env("RI_MKDIR_UMASK_CHILD", mask)
                .output()
                .unwrap();
            assert!(
                o.status.success() && String::from_utf8_lossy(&o.stdout).contains("1 passed"),
                "umask {mask}: {}{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
        }
    }

    /// The process umask from the kernel, without touching it.
    fn umask_now() -> u32 {
        let s = std::fs::read_to_string("/proc/self/status").unwrap();
        let l = s.lines().find(|l| l.starts_with("Umask:")).unwrap();
        u32::from_str_radix(l["Umask:".len()..].trim(), 8).unwrap()
    }

    fn mode_of(p: &Path) -> u32 {
        std::fs::metadata(p).unwrap().mode() & 0o7777
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        let d = std::env::temp_dir().join(format!(
            "qml-files-{}-{}-{name}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        // The umask under test may take u+wx away from the scratch dir itself.
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700)).unwrap();
        d
    }

    /// Runs `child` in a fresh copy of this test binary (the umask is process-wide: other
    /// tests of this binary must not see it change).
    fn in_child(child: &str) {
        if std::env::var_os("QML_FILES_UMASK_CHILD").is_some() {
            return;
        }
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", child, "--nocapture", "--test-threads=1"])
            .env("QML_FILES_UMASK_CHILD", "1")
            .output()
            .unwrap();
        let log = String::from_utf8_lossy(&out.stdout).into_owned()
            + &String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success() && log.contains("1 passed"), "{log}");
    }

    /// mkdir -p / echo > FILE from many job threads at once, while another thread creates
    /// files with mode 0666: the process umask (077) must never change, not even briefly,
    /// and every file and directory gets 0777/0666 & ~077.
    #[test]
    fn concurrent_jobs_never_change_the_umask() {
        in_child("files::tests::concurrent_jobs_never_change_the_umask");
        if std::env::var_os("QML_FILES_UMASK_CHILD").is_none() {
            return;
        }
        // SAFETY: umask(2) cannot fail and only sets the mask of this process, which `in_child`
        // runs for this one test.
        unsafe { libc::umask(0o077) };
        let d = scratch("race");
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let probe = {
            let (d, stop) = (d.clone(), stop.clone());
            std::thread::spawn(move || {
                let mut bad = vec![];
                let mut i = 0;
                while !stop.load(Ordering::SeqCst) {
                    let p = d.join(format!("probe{i}"));
                    let f = std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .mode(0o666)
                        .open(&p)
                        .unwrap();
                    let m = f.metadata().unwrap().mode() & 0o777;
                    if m != 0o600 {
                        bad.push(format!("probe file {m:o}"));
                    }
                    let u = umask_now();
                    if u != 0o077 {
                        bad.push(format!("umask {u:03o}"));
                    }
                    i += 1;
                }
                bad
            })
        };
        let jobs: Vec<_> = (0..8)
            .map(|t| {
                let d = d.clone();
                std::thread::spawn(move || {
                    for i in 0..400 {
                        let p = d.join(format!("t{t}/a{i}/b/c"));
                        mkdir_p(p.to_str().unwrap());
                        let _ = write_line(p.join("f").to_str().unwrap(), "x");
                        // Every thread races to create the same new parents.
                        let s = d.join(format!("shared/s{}/q/t{t}", i % 50));
                        mkdir_p(s.to_str().unwrap());
                        assert!(s.is_dir(), "{} not created", s.display());
                    }
                })
            })
            .collect();
        for j in jobs {
            j.join().unwrap();
        }
        stop.store(true, Ordering::SeqCst);
        let bad = probe.join().unwrap();
        assert_eq!(umask_now(), 0o077, "umask left changed");
        assert!(
            bad.is_empty(),
            "{} bad probes, first {:?}",
            bad.len(),
            &bad[..bad.len().min(5)]
        );
        let mut wrong = vec![];
        let mut stack = vec![d.clone()];
        while let Some(p) = stack.pop() {
            for e in std::fs::read_dir(&p).unwrap().flatten() {
                let p = e.path();
                let m = mode_of(&p);
                let want = if p.is_dir() { 0o700 } else { 0o600 };
                if m != want {
                    wrong.push(format!("{} {m:o}", p.display()));
                }
                if p.is_dir() {
                    stack.push(p);
                }
            }
        }
        assert!(
            wrong.is_empty(),
            "{} wrong modes: {:?}",
            wrong.len(),
            &wrong[..wrong.len().min(5)]
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Modes match coreutils `mkdir -p` and bash `echo > FILE` under the umask current at
    /// each call (it may change between calls: nothing may cache it), including masks that
    /// take u+w/u+x away from the parents.
    #[test]
    fn modes_follow_the_current_umask_like_coreutils() {
        in_child("files::tests::modes_follow_the_current_umask_like_coreutils");
        if std::env::var_os("QML_FILES_UMASK_CHILD").is_none() {
            return;
        }
        for mask in [0o022, 0o077, 0o027, 0o277, 0o177, 0o000, 0o002, 0o022] {
            // SAFETY: umask(2) cannot fail and only sets the mask of this process, which
            // `in_child` runs for this one test.
            unsafe { libc::umask(mask) };
            let d = scratch("modes");
            let (ours, real) = (d.join("ours"), d.join("real"));
            mkdir_p(ours.join("p/q/r").to_str().unwrap());
            let st = std::process::Command::new("mkdir")
                .arg("-p")
                .arg(real.join("p/q/r"))
                .status()
                .unwrap();
            assert!(st.success());
            for sub in ["", "p", "p/q", "p/q/r"] {
                assert_eq!(
                    mode_of(&ours.join(sub)),
                    mode_of(&real.join(sub)),
                    "umask {mask:03o} mkdir -p component {sub:?}"
                );
            }
            // Writable copies of the leaf dirs for the file test (0277 makes them r-x).
            let (fo, fr) = (d.join("fo"), d.join("fr"));
            std::fs::create_dir(&fo).unwrap();
            std::fs::create_dir(&fr).unwrap();
            std::fs::set_permissions(&fo, std::fs::Permissions::from_mode(0o700)).unwrap();
            std::fs::set_permissions(&fr, std::fs::Permissions::from_mode(0o700)).unwrap();
            let _ = write_line(fo.join("f").to_str().unwrap(), "hi");
            let st = std::process::Command::new("bash")
                .arg("-c")
                .arg(format!("echo hi > '{}'", fr.join("f").display()))
                .status()
                .unwrap();
            assert!(st.success());
            assert_eq!(
                mode_of(&fo.join("f")),
                mode_of(&fr.join("f")),
                "umask {mask:03o} echo > FILE"
            );
            assert_eq!(umask_now(), mask, "umask changed by the jobs");
            for p in ["ours", "ours/p", "ours/p/q", "real", "real/p", "real/p/q"] {
                let _ = std::fs::set_permissions(d.join(p), std::fs::Permissions::from_mode(0o700));
            }
            let _ = std::fs::remove_dir_all(&d);
        }
    }

    /// Job threads that mkdir -p under the same new parents, umask without u+wx: a parent is
    /// 0500/0400 until mkdir_p adds u+wx, and no other thread may hit that window (EACCES;
    /// run as non-root, root skips the permission check).
    #[test]
    fn shared_parents_under_umask_without_u_wx() {
        in_child("files::tests::shared_parents_under_umask_without_u_wx");
        if std::env::var_os("QML_FILES_UMASK_CHILD").is_none() {
            return;
        }
        for mask in [0o277u32, 0o377] {
            // SAFETY: umask(2) cannot fail and only sets the mask of this process, which
            // `in_child` runs for this one test.
            unsafe { libc::umask(mask) };
            let d = scratch("shared");
            let js: Vec<_> = (0..8)
                .map(|t| {
                    let d = d.clone();
                    std::thread::spawn(move || {
                        (0..300)
                            .filter(|i| {
                                let s = d.join(format!("s{i}/q/t{t}"));
                                mkdir_p(s.to_str().unwrap());
                                !s.is_dir()
                            })
                            .count()
                    })
                })
                .collect();
            let missing: usize = js.into_iter().map(|j| j.join().unwrap()).sum();
            let _ = std::process::Command::new("chmod")
                .args(["-R", "u+rwx"])
                .arg(&d)
                .status();
            let _ = std::fs::remove_dir_all(&d);
            assert_eq!(missing, 0, "umask {mask:o}: leaves missing");
        }
    }
}
