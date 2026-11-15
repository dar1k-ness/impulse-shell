//! Native backends for Quickshell C++ types: FileView (+ JsonAdapter), SystemClock.

use crate::eval::{Interp, R};
use crate::runtime::{Runtime, World};
use crate::value::Value;
use std::cell::RefCell;
use std::path::Path;

static WAKES: std::sync::Mutex<Vec<(&'static std::panic::Location<'static>, u32)>> =
    std::sync::Mutex::new(Vec::new());

pub(crate) fn note_wake(l: &'static std::panic::Location<'static>) {
    if !crate::native::wake_stats_on() {
        return;
    }
    let mut v = WAKES.lock().unwrap();
    match v.iter_mut().find(|e| std::ptr::eq(e.0, l)) {
        Some(e) => e.1 += 1,
        None => v.push((l, 1)),
    }
}

/// Effects nothing consumes are dropped (they used to pile up and keep the loop polling).
pub(crate) fn unhandled_effect(k: &str) {
    static SEEN: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    let mut s = SEEN.lock().unwrap();
    if !s.iter().any(|x| x == k) {
        elog::eprintln!("native: no handler for effect {k}");
        s.push(k.to_string());
    }
}

/// Wall-clock ms (mod 1e6) for correlating ri::tracew lines across threads.
pub fn trace_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() % 1_000_000)
}

static POLLT: std::sync::Mutex<Vec<(&'static str, f64)>> = std::sync::Mutex::new(Vec::new());

struct Lap(Option<std::time::Instant>);

impl Lap {
    fn new() -> Lap {
        let l = Lap(wake_stats_on().then(std::time::Instant::now));
        l.0.is_some().then(|| note_poll("calls", 0.0));
        l
    }
    fn at(&mut self, k: &'static str) {
        if let Some(t) = self.0 {
            note_poll(k, t.elapsed().as_secs_f64() * 1000.0);
            self.0 = Some(std::time::Instant::now());
        }
    }
}

fn note_poll(k: &'static str, ms: f64) {
    let mut v = POLLT.lock().unwrap();
    let add = if k == "calls" { 1.0 } else { ms };
    match v.iter_mut().find(|e| e.0 == k) {
        Some(e) => e.1 += add,
        None => v.push((k, add)),
    }
}

fn wake_stats_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| crate::log_on!("ri::stats"))
}

/// Wakeups by helper threads since the last call, per call site (ri::stats only).
pub fn take_wake_stats() -> String {
    let mut v = std::mem::take(&mut *WAKES.lock().unwrap());
    v.sort_by_key(|e| std::cmp::Reverse(e.1));
    v.iter()
        .map(|(l, n)| {
            format!(
                "{}:{}={n}",
                l.file().rsplit('/').next().unwrap_or(""),
                l.line()
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
        + " | poll "
        + &std::mem::take(&mut *POLLT.lock().unwrap())
            .iter()
            .map(|(k, v)| format!("{k}={v:.1}"))
            .collect::<Vec<_>>()
            .join(" ")
}

fn live_of(w: &mut World, ty: &str) -> Vec<usize> {
    w.of_type(ty)
}

fn assign_json(w: &mut World, a: usize, v: &Value) {
    let Value::Obj(o) = v else { return };
    let pairs: Vec<(String, Value)> = o
        .borrow()
        .props
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect();
    for (k, val) in pairs {
        let Some(cur) = w.items[a].props.get(&k).map(|p| p.value.clone()) else {
            continue;
        };
        match (cur, &val) {
            (Value::Item(c), Value::Obj(_))
                if w.items[c].types.iter().any(|t| t == "JsonObject") =>
            {
                assign_json(w, c, &val)
            }
            // A sequence/map property only notifies when its contents change (QMetaType::equals).
            (cur, _) if json_same(&cur, &val) => {}
            _ => {
                w.write(a, &k, val);
            }
        }
    }
}

pub(crate) fn json_same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Obj(x), Value::Obj(y)) => {
            if std::rc::Rc::ptr_eq(x, y) {
                return true;
            }
            let (x, y) = (x.borrow(), y.borrow());
            if x.call.is_some() || y.call.is_some() {
                return false;
            }
            match (&x.array, &y.array) {
                (Some(p), Some(q)) => {
                    p.len() == q.len() && p.iter().zip(q.iter()).all(|(u, v)| json_same(u, v))
                }
                (None, None) => {
                    x.props.len() == y.props.len()
                        && x.props
                            .iter()
                            .all(|(k, u)| y.props.get(k).is_some_and(|v| json_same(u, v)))
                }
                _ => false,
            }
        }
        _ => a.strict_eq(b),
    }
}

fn file_path(w: &mut World, i: usize) -> String {
    if let Some(v) = w.file_views.get(&i) {
        return v.target.clone();
    }
    let path = match w.read(i, "path") {
        Some(Value::Str(s)) => s.to_string(),
        _ => String::new(),
    };
    crate::qurl::trim_file_protocol(&path).to_string()
}

pub type ReadHook = Box<dyn FnMut(&str) -> Option<String>>;

thread_local!(static READ_HOOK: std::cell::RefCell<Option<ReadHook>> = const { std::cell::RefCell::new(None) });

/// Replace this thread's FileView reads (differential checks record/replay their inputs).
pub fn set_read_hook(h: Option<ReadHook>) {
    READ_HOOK.with(|r| *r.borrow_mut() = h);
}

/// Replaces running commands on this thread: gets the argv, returns the stdout of a
/// successful run (`None`: the command fails). Differential checks drive handlers that would
/// otherwise launch apps or power off the machine.
pub type SpawnHook = Box<dyn FnMut(&[String]) -> Option<String>>;

thread_local!(static SPAWN_HOOK: std::cell::RefCell<Option<SpawnHook>> = const { std::cell::RefCell::new(None) });
thread_local!(static NO_WRITES: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });

/// Sandbox this thread's `Process`/`execDetached`/`DesktopEntry.execute` (see [`SpawnHook`]).
pub fn set_spawn_hook(h: Option<SpawnHook>) {
    SPAWN_HOOK.with(|r| *r.borrow_mut() = h);
}

/// Drop this thread's FileView writes.
pub fn set_no_writes(on: bool) {
    NO_WRITES.with(|c| c.set(on));
}

/// Commands are sandboxed (aotcheck): services must not touch the real system either.
pub(crate) fn sandboxed() -> bool {
    SPAWN_HOOK.with(|r| r.borrow().is_some())
}

/// `Some(result)` when a [`SpawnHook`] replaces running `argv`.
pub(crate) fn hooked_spawn(argv: &[String]) -> Option<Option<String>> {
    SPAWN_HOOK.with(|r| r.borrow_mut().as_mut().map(|h| h(argv)))
}

/// [`hooked_spawn`] for a call of an `Impulse` type that runs no command: the hook gets the
/// type's QML name, the method and its arguments as text. An element that is a job gets
/// [`crate::impulse::RUN`] for the method and its property values for the arguments.
pub(crate) fn hooked_call(ty: &str, method: &str, args: &[&str]) -> Option<Option<String>> {
    let argv: Vec<String> = [ty, method]
        .iter()
        .chain(args)
        .map(|s| s.to_string())
        .collect();
    hooked_spawn(&argv)
}

/// Most a FileView reads. Configs and `/proc` files are far smaller; a huge file would stall
/// the UI thread and hold its whole text.
const FILEVIEW_MAX: u64 = 64 << 20;

/// A FileView's text: only a regular file is read (a FIFO blocked the UI thread for good,
/// `/dev/zero` grew the shell until OOM), at most [`FILEVIEW_MAX`] bytes.
fn read_input(path: &str) -> Option<String> {
    READ_HOOK
        .with(|r| r.borrow_mut().as_mut().map(|h| h(path)))
        .unwrap_or_else(|| {
            crate::osutil::read_regular(std::path::Path::new(path), FILEVIEW_MAX)
                .ok()
                .and_then(|b| String::from_utf8(b).ok())
        })
}

/// A signal of a FileView and its arguments.
pub(crate) type FileSaid = (&'static str, Vec<Value>);

/// FileView::setPath: another path is read at once in the background when the view
/// preloads; when not, what was being read is dropped and the text is said to have changed.
pub(crate) fn file_set_path(w: &mut World, i: usize) {
    let p = match w.items[i].props.get("__path").map(|p| &p.value) {
        Some(Value::Str(s)) => crate::qurl::trim_file_protocol(s).to_string(),
        _ => String::new(),
    };
    let v = w.file_views.entry(i).or_default();
    if v.target == p {
        return;
    }
    v.target = p;
    file_reload(w, i);
}

/// FileView::reload (updatePath): the file is read again, in the background when the view
/// preloads and by the next reader of its text when not.
pub(crate) fn file_reload(w: &mut World, i: usize) -> Option<FileSaid> {
    w.file_loaded.remove(&i);
    let preload = !matches!(w.read(i, "__preload"), Some(Value::Bool(false)));
    let v = w.file_views.get_mut(&i)?;
    v.job = preload && !v.target.is_empty();
    if !v.job {
        w.notify(i, "text");
    }
    None
}

/// Whether the text of the view `i` is not that of its path as it was last asked for.
fn file_stale(w: &mut World, i: usize) -> bool {
    let path = file_path(w, i);
    !path.is_empty() && w.file_loaded.get(&i) != Some(&path)
}

/// FileView::waitForJob: the read under way ends before the call does; `None` without one.
pub(crate) fn file_wait(w: &mut World, i: usize, sync: bool) -> Option<Option<FileSaid>> {
    let job = w.file_views.get(&i).is_none_or(|v| v.job);
    (job && file_stale(w, i)).then(|| load_file_said(w, i, sync))
}

/// FileView::text and data: a view that blocks reads the file before it answers, another
/// one starts reading it.
pub(crate) fn file_prepare(w: &mut World, i: usize, sync: bool) -> Option<FileSaid> {
    if !file_stale(w, i) {
        return None;
    }
    let on = |w: &mut World, name: &str| w.read(i, name).is_some_and(|v| v.truthy());
    if on(w, "__blockAllReads") || on(w, "__blockLoading") && !on(w, "loaded") {
        return load_file_said(w, i, sync);
    }
    if let Some(v) = w.file_views.get_mut(&i) {
        v.job = true;
    }
    None
}

/// Synchronously (re)read a FileView's file, as Quickshell's blocking `reload()` + `text()`.
pub(crate) fn load_file(w: &mut World, i: usize) {
    load_file_said(w, i, false);
}

/// [`load_file`]; with `sync` the signal of its outcome is left to the caller to emit.
fn load_file_said(w: &mut World, i: usize, sync: bool) -> Option<FileSaid> {
    let path = file_path(w, i);
    if path.is_empty() {
        return None;
    }
    w.file_loaded.insert(i, path.clone());
    if let Some(v) = w.file_views.get_mut(&i) {
        v.job = false;
    }
    let Some(text) = read_input(&path) else {
        // qs::io::FileViewError
        let p = std::path::Path::new(&path);
        // Never `open` a FIFO here: that blocks like the read did.
        let code = match std::fs::metadata(p) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => 2.0,
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => 3.0,
            Ok(m) if !m.is_file() => 4.0,
            Ok(_) => match std::fs::File::open(p) {
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => 3.0,
                _ => 1.0,
            },
            Err(_) => 1.0,
        };
        w.notify(i, "text");
        if sync {
            (w.signals.entry(i).or_default())
                .entry("loadFailed".to_string())
                .or_insert_with(|| vec![crate::runtime::SigParam::untyped("error")]);
            return Some(("loadFailed", vec![Value::Num(code)]));
        }
        emit(w, i, "loadFailed", &["error"], vec![Value::Num(code)]);
        return None;
    };
    let adapter = match w.items[i].props.get("adapter").map(|p| p.value.clone()) {
        Some(Value::Item(a)) => Some(a),
        _ => w.items[i]
            .children
            .iter()
            .copied()
            .find(|&c| w.items[c].types.iter().any(|t| t == "JsonAdapter")),
    };
    // Text from someone else replaces the view's own last write: that write coming back
    // later (another process restoring the same options) is a change, not an echo.
    if w.file_written.get(&i).is_some_and(|t| *t != text) {
        w.file_written.remove(&i);
    }
    if !w.file_views.contains_key(&i) {
        w.write(i, "__path", Value::str(&path));
        w.file_views.remove(&i);
    }
    w.write(i, "__text", Value::str(&text));
    w.write(i, "__data", Value::str(&text));
    if let Some(a) = adapter
        && let Some(v) = crate::json::parse(&text).ok()
    {
        w.json_loading = true;
        assign_json(w, a, &v);
        w.json_loading = false;
    }
    w.write(i, "loaded", Value::Bool(true));
    w.notify(i, "text");
    if sync {
        return Some(("loaded", Vec::new()));
    }
    w.fire.push(crate::runtime::Fire::Handler(
        i,
        crate::runtime::Sym::new("onLoaded"),
    ));
    None
}

/// The watch event of a FileView's own write: the file still holds exactly what it wrote.
/// Quickshell reports it as fileChanged, and the original's `onFileChanged: reload()` (Config.qml) then
/// reverts edits made since that write, e.g. keystrokes typed into a settings text field.
fn own_write_echo(w: &World, i: usize, path: &str) -> bool {
    w.file_written
        .get(&i)
        .is_some_and(|t| read_input(path).as_deref() == Some(t.as_str()))
}

/// The file holds exactly the text the view last loaded.
fn same_text(w: &mut World, i: usize, path: &str) -> bool {
    w.file_loaded.get(&i).is_some_and(|p| p == path)
        && w.read(i, "loaded").is_some_and(|v| v.truthy())
        && read_input(path)
            .is_some_and(|t| w.read(i, "__text").is_some_and(|v| v.to_js_string() == t))
}

fn adapter_of(w: &World, i: usize) -> Option<usize> {
    match w.items[i].props.get("adapter").map(|p| p.value.clone()) {
        Some(Value::Item(a)) => Some(a),
        _ => w.items[i]
            .children
            .iter()
            .copied()
            .find(|&c| w.items[c].types.iter().any(|t| t == "JsonAdapter")),
    }
}

/// Quickshell `atomicWrites` (default on): write a sibling temp file, then rename over.
fn write_file(w: &mut World, i: usize, text: &str) {
    if NO_WRITES.with(|c| c.get()) || w.read(i, "blockWrites").is_some_and(|v| v.truthy()) {
        return;
    }
    let path = file_path(w, i);
    if path.is_empty() {
        return;
    }
    let p = std::path::Path::new(&path);
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let atomic = w.read(i, "atomicWrites").is_none_or(|v| v.truthy());
    let ok = if atomic {
        save_file(p, text)
    } else {
        std::fs::write(&path, text)
    };
    if let Err(e) = ok {
        elog::eprintln!("FileView: cannot write {path}: {e}");
        return;
    }
    w.file_written.insert(i, text.to_string());
    w.write(i, "__text", Value::str(text));
    w.write(i, "__data", Value::str(text));
    w.fire.push(crate::runtime::Fire::Handler(
        i,
        crate::runtime::Sym::new("onSaved"),
    ));
}

/// Whether two JSON values are the same setting (`1` and `1.0` are).
fn json_text_same(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    use serde_json::Value as J;
    match (a, b) {
        (J::Number(x), J::Number(y)) => x.as_f64() == y.as_f64(),
        (J::Array(x), J::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(x, y)| json_text_same(x, y))
        }
        (J::Object(x), J::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, x)| y.get(k).is_some_and(|y| json_text_same(x, y)))
        }
        _ => a == b,
    }
}

/// Takes into the adapter `a` every value of `disk` that differs from `base` where the
/// adapter (`ours`) still holds the value of `base`.
fn take_foreign_values(
    w: &mut World,
    a: usize,
    base: &serde_json::Value,
    disk: &serde_json::Value,
    ours: &serde_json::Value,
) {
    let (Some(b), Some(d), Some(o)) = (base.as_object(), disk.as_object(), ours.as_object()) else {
        return;
    };
    for (k, dv) in d {
        let (Some(bv), Some(ov)) = (b.get(k), o.get(k)) else {
            continue;
        };
        if json_text_same(bv, dv) {
            continue;
        }
        match w.items[a].props.get(k.as_str()).map(|p| p.value.clone()) {
            Some(Value::Item(c)) if w.items[c].types.iter().any(|t| t == "JsonObject") => {
                take_foreign_values(w, c, bv, dv, ov)
            }
            Some(_) if json_text_same(ov, bv) => {
                if let Ok(v) = crate::json::parse(&dv.to_string()) {
                    w.write(a, k, v);
                }
            }
            _ => {}
        }
    }
}

/// A watching view whose file no longer holds the text the view last read or wrote: someone
/// else saved it and the view's reload has not run yet. The adapter takes the values that
/// writer changed and the view left alone, so that the save about to follow does not put the
/// view's older copy of them back. The original's shell and its settings window are two processes that
/// each save all of config.json 50 ms after an edit: the shell saving a new `wallpaperPath`
/// took the accent just picked in settings back to the one it had loaded.
fn merge_foreign_save(w: &mut World, i: usize, a: usize) {
    if !w.read(i, "watchChanges").is_some_and(|v| v.truthy()) {
        return;
    }
    let Some(Value::Str(base)) = w.read(i, "__text") else {
        return;
    };
    let Some(disk) = read_input(&file_path(w, i)) else {
        return;
    };
    if *disk == *base {
        return;
    }
    let json = |t: &str| serde_json::from_str::<serde_json::Value>(t).ok();
    let (Some(base), Some(disk), Some(ours)) = (
        json(&base),
        json(&disk),
        json(&crate::json::adapter_text(w, a)),
    ) else {
        return;
    };
    let loading = std::mem::replace(&mut w.json_loading, true);
    take_foreign_values(w, a, &base, &disk, &ours);
    w.json_loading = loading;
}

pub(crate) fn write_adapter(w: &mut World, i: usize) {
    let Some(a) = adapter_of(w, i) else { return };
    w.adapter_unsaved.remove(&i);
    merge_foreign_save(w, i, a);
    let out = crate::json::adapter_text(w, a);
    write_file(w, i, &out);
}

pub(crate) fn set_text(w: &mut World, i: usize, text: &str) {
    write_file(w, i, text);
}

pub enum ProcEv {
    Out(String),
    Err(String),
    /// A normal exit with this code (QProcess::NormalExit).
    Exit(i32),
    /// Ended by this signal (QProcess::CrashExit, `exitCode` = the signal).
    Signaled(i32),
    /// A thumbnail run (`crate::imgjobs`) is done with `path`, the `done`th of its `total` files.
    File {
        done: usize,
        total: usize,
        path: String,
    },
}

/// A spawned `Process`: its pipes and pidfd (polled through `World::proc_ep`), the events
/// read from them, plus unflushed SplitParser data.
pub struct Proc {
    pid: i32,
    /// Set for in-process command implementations (pid 0) to stop them.
    stop: Option<std::sync::Arc<crate::svc::Stop>>,
    queued: std::collections::VecDeque<ProcEv>,
    /// Events from an in-process job (`crate::jobs`), which wakes the loop itself.
    job: Option<std::sync::mpsc::Receiver<ProcEv>>,
    io: Option<ProcIo>,
    out: String,
    err: String,
    /// `running = true` arrived while alive: start again once this one exits.
    restart: bool,
    /// The child's stdin (`stdinEnabled`); dropping it closes the pipe.
    stdin: Option<Stdin>,
    /// In-process image job under a frame-pacing host: `World::frame_seq` when it started. Its
    /// output waits for its window's next frame, as the real script's always does: the handlers
    /// read laid-out sizes (the background clock centers on the least busy region with
    /// `width / 2`). A job outside any window waits for the next frame of any window. Output
    /// no window can draw a frame for (`World::frame_wins`) is not held.
    after_frame: Option<u64>,
}

/// The child's stdout/stderr (non-blocking) and pidfd, registered in the World's epoll set
/// under `item << 2 | k` (k: 0 stdout, 1 stderr, 2 exit; 3 is [`Stdin`]).
struct ProcIo {
    ch: Option<std::process::Child>,
    pipes: [Option<std::os::fd::OwnedFd>; 2],
    pend: [Vec<u8>; 2],
    pidfd: Option<std::os::fd::OwnedFd>,
}

/// A Process destroyed while its child runs (item destruction, the World's drop): as
/// `Proc::kill_group`, SIGKILL the group and free its shutdown slot before the reaper takes it.
impl Drop for ProcIo {
    fn drop(&mut self) {
        if let Some(ch) = self.ch.take() {
            let pid = ch.id() as i32;
            // SAFETY: kill(2) takes plain integers. Unreaped, so the group id is still the
            // child's.
            unsafe { libc::kill(-pid, libc::SIGKILL) };
            children::remove(pid);
            crate::childguard::remove(pid);
            workpool::reap(ch);
        }
    }
}

/// QProcess::~QProcess kills a still running process: an in-process job still running (also
/// one only terminated, whose programs may ignore SIGTERM) is killed as `kill_group` does.
impl Drop for Proc {
    fn drop(&mut self) {
        if let Some(s) = &self.stop {
            s.kill();
        }
    }
}

/// Key of the `crate::fswatch` inotify fd in the World's epoll set (no Process key: those are
/// `item << 2 | k`, k <= 3).
pub(crate) const EP_FSWATCH: u64 = u64::MAX;

/// The write end of a child's stdin (non-blocking). What the pipe does not take at once is
/// kept and written when the World's epoll set reports it writable (`EPOLLOUT` under
/// `item << 2 | 3`, registered only while data waits), so a child that does not read never
/// blocks the shell or holds a thread.
struct Stdin {
    fd: std::os::fd::OwnedFd,
    buf: std::collections::VecDeque<u8>,
    armed: bool,
    /// `stdinEnabled = false` while data waited: close once it is written.
    closing: bool,
    ep: i32,
    key: u64,
}

impl Stdin {
    fn new(pipe: std::process::ChildStdin, ep: i32, key: u64) -> Stdin {
        use std::os::fd::AsRawFd;
        let fd = std::os::fd::OwnedFd::from(pipe);
        set_nonblocking(fd.as_raw_fd());
        Stdin {
            fd,
            buf: Default::default(),
            armed: false,
            closing: false,
            ep,
            key: key | 3,
        }
    }

    /// Writes what the pipe takes; `false` once the pipe is done with (closed by the child,
    /// or drained after `stdinEnabled = false`): drop it.
    fn flush(&mut self) -> bool {
        use std::os::fd::AsRawFd;
        while !self.buf.is_empty() {
            let (a, _) = self.buf.as_slices();
            // SAFETY: the fd is the open pipe `self.fd`; `a` is a live slice of `a.len()` bytes.
            let n = unsafe { libc::write(self.fd.as_raw_fd(), a.as_ptr().cast(), a.len()) };
            if n >= 0 {
                self.buf.drain(..n as usize);
                continue;
            }
            match std::io::Error::last_os_error().kind() {
                std::io::ErrorKind::Interrupted => continue,
                std::io::ErrorKind::WouldBlock => return self.arm(true),
                // EPIPE: the child closed its stdin or exited; QProcess drops the data too.
                _ => return false,
            }
        }
        self.arm(false) && !self.closing
    }

    fn arm(&mut self, on: bool) -> bool {
        use std::os::fd::AsRawFd;
        if on == self.armed {
            return true;
        }
        let mut ev = libc::epoll_event {
            events: libc::EPOLLOUT as u32,
            u64: self.key,
        };
        let op = if on {
            libc::EPOLL_CTL_ADD
        } else {
            libc::EPOLL_CTL_DEL
        };
        // SAFETY: `ev` is a live epoll_event for the call; bad fds only fail with an error.
        if unsafe { libc::epoll_ctl(self.ep, op, self.fd.as_raw_fd(), &mut ev) } != 0 {
            elog::eprintln!(
                "Process: stdin: epoll_ctl: {}",
                std::io::Error::last_os_error()
            );
            return false;
        }
        self.armed = on;
        true
    }
}

fn set_nonblocking(fd: i32) {
    // SAFETY: F_GETFL takes no argument; a bad fd only fails with an error.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    // SAFETY: F_SETFL takes an int argument; a bad fd only fails with an error.
    unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
}

pub(crate) fn ep_add(ep: i32, fd: i32, key: u64) -> bool {
    let mut ev = libc::epoll_event {
        events: libc::EPOLLIN as u32,
        u64: key,
    };
    // SAFETY: `ev` is a live epoll_event for the call; bad fds only fail with an error.
    unsafe { libc::epoll_ctl(ep, libc::EPOLL_CTL_ADD, fd, &mut ev) == 0 }
}

impl ProcIo {
    /// Read what pipe `k` has (bounded, the level-triggered set reports the rest again); `k`
    /// 2 is the pidfd: the child died.
    fn pump(&mut self, k: usize, out: &mut std::collections::VecDeque<ProcEv>) {
        if k == 2 {
            self.died(out);
            return;
        }
        self.read(k, 16 * 8192, out);
        if self.pipes.iter().all(Option::is_none) {
            self.reap(out);
        }
    }

    /// Reads up to `budget` bytes of pipe `k`, closing it at EOF.
    fn read(&mut self, k: usize, mut budget: usize, out: &mut std::collections::VecDeque<ProcEv>) {
        use std::os::fd::AsRawFd;
        let mut buf = [0u8; 8192];
        while budget > 0 {
            let Some(fd) = self.pipes[k].as_ref() else {
                return;
            };
            let want = budget.min(buf.len());
            // SAFETY: the fd is the open pipe `fd`; `buf` is a live buffer of at least `want`
            // bytes.
            let n = unsafe { libc::read(fd.as_raw_fd(), buf.as_mut_ptr().cast(), want) };
            if n < 0 {
                match std::io::Error::last_os_error().kind() {
                    std::io::ErrorKind::Interrupted => continue,
                    std::io::ErrorKind::WouldBlock => return,
                    _ => {}
                }
            }
            if n <= 0 {
                self.pipes[k] = None;
                self.flush(k, true, out);
                return;
            }
            budget -= n as usize;
            self.pend[k].extend_from_slice(&buf[..n as usize]);
            self.flush(k, false, out);
        }
    }

    /// Forwards the complete UTF-8 sequences pending on pipe `k` (`all`: the rest too).
    fn flush(&mut self, k: usize, all: bool, out: &mut std::collections::VecDeque<ProcEv>) {
        let pend = &mut self.pend[k];
        let cut = match std::str::from_utf8(pend) {
            Ok(_) => pend.len(),
            Err(e) if e.error_len().is_none() && !all => e.valid_up_to(),
            Err(_) => pend.len(),
        };
        if cut == 0 {
            return;
        }
        let chunk = String::from_utf8_lossy(&pend[..cut]).into_owned();
        pend.drain(..cut);
        out.push_back(if k == 0 {
            ProcEv::Out(chunk)
        } else {
            ProcEv::Err(chunk)
        });
    }

    /// The pidfd is readable, as QProcess's `_q_processDied`: what the pipes hold now is read,
    /// then they are closed and the child reaped, though a descendant still has them open.
    fn died(&mut self, out: &mut std::collections::VecDeque<ProcEv>) {
        use std::os::fd::AsRawFd;
        for k in 0..2 {
            let Some(fd) = self.pipes[k].as_ref() else {
                continue;
            };
            let mut n: libc::c_int = 0;
            // SAFETY: FIONREAD writes one int, `n`, for the open pipe `fd`.
            if unsafe { libc::ioctl(fd.as_raw_fd(), libc::FIONREAD, &mut n) } == 0 && n > 0 {
                self.read(k, n as usize, out);
            }
            self.pipes[k] = None;
            self.flush(k, true, out);
        }
        self.reap(out);
    }

    /// Reaps the child once it exited (the pidfd reports it otherwise) and reports how.
    fn reap(&mut self, out: &mut std::collections::VecDeque<ProcEv>) {
        use std::os::unix::process::ExitStatusExt;
        let Some(ch) = self.ch.as_mut() else { return };
        let pid = ch.id();
        let polled = if untrack(pid as i32) {
            ch.try_wait()
        } else {
            Ok(None)
        };
        let st = match polled {
            Ok(None) => return,
            Ok(s) => s,
            // The child is gone, how it ended is unknown (`ECHILD`: something else reaped
            // it, or SIGCHLD is ignored): `exited` is still emitted, with exit code -1.
            Err(e) => {
                elog::eprintln!("impulse-shell: cannot wait for child {pid}: {e}");
                None
            }
        };
        self.ch = None;
        self.pidfd = None;
        self.pipes = [None, None];
        out.push_back(match st.map(|s| (s.code(), s.signal())) {
            Some((Some(c), _)) => ProcEv::Exit(c),
            Some((None, Some(sig))) => ProcEv::Signaled(sig),
            _ => ProcEv::Exit(-1),
        });
    }
}

/// Untrack an exited child before reaping it: once reaped, its pid (and group id) may be reused.
fn untrack(pid: i32) -> bool {
    // SAFETY: siginfo_t is integers and pointers, for which all zeros (null) is valid.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let flags = libc::WEXITED | libc::WNOWAIT | libc::WNOHANG;
    loop {
        // SAFETY: `info` is a live siginfo_t for the kernel to fill.
        let r = unsafe { libc::waitid(libc::P_PID, pid as libc::id_t, &mut info, flags) };
        if r < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        // SAFETY: `info` is initialized (zeroed, then filled by waitid(2) as a SIGCHLD siginfo
        // when it succeeds), so its si_pid field is readable.
        if r < 0 || unsafe { info.si_pid() } != pid {
            return r < 0;
        }
        children::remove(pid);
        crate::childguard::remove(pid);
        return true;
    }
}

impl Proc {
    /// The child's pid while it is not reaped: only then is the id (and its group's) still the
    /// child's. Once [`ProcIo::reap`] has taken its status the kernel may give the id to any
    /// new process, though this Process's exit event is still queued.
    fn unreaped(&self) -> Option<i32> {
        self.io
            .as_ref()
            .is_some_and(|io| io.ch.is_some())
            .then_some(self.pid)
            .filter(|pid| *pid > 0)
    }

    /// A destroyed Process: SIGKILL its whole group, not just the leader (QProcess::kill
    /// orphans a `sh -c` pipeline's or `&` job's members) and free its shutdown slot. Nothing
    /// is signalled once the leader is reaped ([`Proc::unreaped`]). An in-process job is killed
    /// with the groups of the programs it runs.
    pub(crate) fn kill_group(&self) {
        if let Some(pid) = self.unreaped() {
            send_signal(-pid, libc::SIGKILL);
            children::remove(pid);
        }
        self.kill(libc::SIGKILL);
    }

    /// An in-process job's `Exit` (or its thread's end) arrived; drops the rest of its output.
    fn job_ended(&self) -> bool {
        let Some(rx) = &self.job else { return true };
        loop {
            match rx.try_recv() {
                Ok(ProcEv::Exit(_) | ProcEv::Signaled(_))
                | Err(std::sync::mpsc::TryRecvError::Disconnected) => return true,
                Ok(_) => {}
                Err(std::sync::mpsc::TryRecvError::Empty) => return false,
            }
        }
    }

    pub fn pid(&self) -> i32 {
        self.pid
    }

    pub(crate) fn close_stdin(&mut self) {
        if let Some(s) = self.stdin.as_mut() {
            s.closing = true;
        }
        self.flush_stdin();
    }

    fn flush_stdin(&mut self) {
        if self.stdin.as_mut().is_some_and(|s| !s.flush()) {
            self.stdin = None;
        }
    }

    /// Process.signal(sig): the child gets `sig`, unless it is reaped already
    /// ([`Proc::unreaped`]). An in-process job is stopped; for SIGKILL (QProcess::kill) the
    /// groups of the programs it runs are SIGKILLed too.
    pub fn kill(&self, sig: i32) {
        if let Some(s) = &self.stop {
            if sig == libc::SIGKILL {
                s.kill();
            } else {
                s.store(true, std::sync::atomic::Ordering::Release);
            }
        }
        if let Some(pid) = self.unreaped() {
            send_signal(pid, sig);
        }
    }
}

/// kill(2) for a `Process`'s child (`pid`) or its group (`-pid`).
fn send_signal(pid: i32, sig: i32) {
    #[cfg(test)]
    SIGNALS_SENT.with_borrow_mut(|s| s.push((pid, sig)));
    // SAFETY: kill(2) takes plain integers and touches no memory of this process.
    unsafe { libc::kill(pid, sig) };
}

#[cfg(test)]
thread_local! {
    /// Every [`send_signal`] of this thread, in order (tests).
    static SIGNALS_SENT: std::cell::RefCell<Vec<(i32, i32)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// An in-process job's `running = false` (QProcess::terminate): it is stopped and, until it
/// ends, kept in `World::procs_ending` so the Process's destruction or the Runtime's drop
/// still kills what it runs, as ~QProcess kills a child that ignored the SIGTERM.
pub(crate) fn terminate_job(w: &mut World, p: usize, pr: Proc) {
    pr.kill(libc::SIGTERM);
    if pr.stop.is_some() && !pr.job_ended() {
        w.procs_ending.push((p, pr));
    }
}

/// `running = run` on a Process whose child is alive, as Quickshell's Process::setRunning: false
/// only sends SIGTERM (the process keeps `running` until it exits), true asks for a new start once
/// it has exited. Returns false when the write should be stored as usual.
pub(crate) fn running_write(w: &mut World, p: usize, run: bool) -> bool {
    let Some(pr) = w.procs.get_mut(&p) else {
        return false;
    };
    // In-process command implementations stop at once, with the exit Quickshell reports for a
    // terminate()d child (15, CrashExit) queued, so a following `running = true` (Translator's
    // `running = false; running = true`) starts a fresh job instead of keeping the stale one.
    if pr.pid == 0 {
        if !run {
            let pr = w.procs.remove(&p).unwrap();
            terminate_job(w, p, pr);
            if crate::imgjobs::is_type(w, p) {
                crate::imgjobs::finished(w, p);
                return false;
            }
            w.write(p, "processId", Value::Null);
            emit(
                w,
                p,
                "exited",
                &["exitCode", "exitStatus"],
                vec![Value::Num(libc::SIGTERM as f64), Value::Num(1.0)],
            );
        }
        return false;
    }
    pr.restart = run;
    if !run {
        pr.kill(libc::SIGTERM);
    }
    true
}

fn str_list(v: &Value) -> Vec<String> {
    match v {
        Value::Obj(o) => o
            .borrow()
            .array
            .as_ref()
            .map(|a| a.iter().map(|x| x.to_js_string()).collect())
            .unwrap_or_default(),
        Value::Str(s) => vec![s.to_string()],
        _ => vec![],
    }
}

/// Running `Process` children and the programs in-process jobs and [`crate::child`] start.
/// Each leads its own process group, which gets SIGTERM when the shell exits (exit(), return
/// from main, a fatal or terminating signal), so a `bash -c` pipeline or `nmcli monitor` does
/// not outlive it, and SIGKILL on a reload ([`end_handed_over`]). Slots are atomics so the
/// signal handler can walk them.
///
/// A slot holds `pid` while the child runs and `-pid` while a signaller holds it: the group
/// is signalled, then the slot freed. [`remove`] (called before the child is reaped, on any
/// thread) waits on the slot's futex until such a signaller is done, so no group id is
/// signalled after its leader was reaped and the id may have been reused.
pub(crate) mod children {
    use std::sync::atomic::{AtomicI32, Ordering};

    const SLOTS: usize = 512;
    static PIDS: [AtomicI32; SLOTS] = [const { AtomicI32::new(0) }; SLOTS];
    static OLD: std::sync::OnceLock<Vec<(i32, libc::sigaction)>> = std::sync::OnceLock::new();
    /// Signals whose delivery ends the process: terminate the children first, then let the
    /// previous disposition (default, or std's stack-overflow handler for SEGV/BUS) act.
    const FATAL: [i32; 9] = [
        libc::SIGTERM,
        libc::SIGINT,
        libc::SIGHUP,
        libc::SIGQUIT,
        libc::SIGABRT,
        libc::SIGSEGV,
        libc::SIGBUS,
        libc::SIGILL,
        libc::SIGFPE,
    ];

    pub(crate) fn add(pid: i32) {
        install();
        if !PIDS.iter().any(|s| {
            s.compare_exchange(0, pid, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
        }) {
            elog::eprintln!("Process {pid}: not tracked for shutdown ({SLOTS} running)");
        }
    }

    #[cfg(test)]
    pub(super) fn tracked(pid: i32) -> bool {
        PIDS.iter().any(|s| s.load(Ordering::Acquire) == pid)
    }

    /// Frees `pid`'s slot; call before reaping it. Returns once no signaller holds it.
    pub(crate) fn remove(pid: i32) {
        for s in &PIDS {
            loop {
                match s.compare_exchange(pid, 0, Ordering::AcqRel, Ordering::Acquire) {
                    Ok(_) => return,
                    Err(v) if v == -pid => futex_wait(s, v),
                    // Freed by the signaller, or another child's slot.
                    Err(_) => break,
                }
            }
        }
    }

    /// Blocks while `s` holds `v` (futex(2) FUTEX_WAIT; a changed value returns at once).
    fn futex_wait(s: &AtomicI32, v: i32) {
        // SAFETY: `s` is a live, aligned 32-bit futex word; the null timeout waits until a wake.
        unsafe {
            libc::syscall(
                libc::SYS_futex,
                s.as_ptr(),
                libc::FUTEX_WAIT | libc::FUTEX_PRIVATE_FLAG,
                v,
                std::ptr::null::<libc::timespec>(),
            )
        };
    }

    /// Runs `f` with every tracked group while its slot is held (its leader cannot be reaped
    /// meanwhile), freeing the slot when `f` returns false. Async-signal-safe when `f` is.
    fn each_held(mut f: impl FnMut(i32) -> bool) {
        for s in &PIDS {
            let pid = s.load(Ordering::Acquire);
            if pid <= 0
                || s.compare_exchange(pid, -pid, Ordering::AcqRel, Ordering::Relaxed)
                    .is_err()
            {
                continue;
            }
            let keep = f(pid);
            s.store(if keep { pid } else { 0 }, Ordering::Release);
            // SAFETY: as for `futex_wait`; FUTEX_WAKE wakes every waiter and is a plain
            // syscall, async-signal-safe.
            unsafe {
                libc::syscall(
                    libc::SYS_futex,
                    s.as_ptr(),
                    libc::FUTEX_WAKE | libc::FUTEX_PRIVATE_FLAG,
                    i32::MAX,
                )
            };
        }
    }

    /// Sends `sig` to every tracked group and frees its slot; `then` runs with each group
    /// signalled while its leader cannot have been reaped yet. Async-signal-safe when `then`
    /// is.
    fn signal_all(sig: i32, mut then: impl FnMut(i32)) {
        each_held(|pid| {
            // SAFETY: kill(2) takes plain integers and is async-signal-safe.
            unsafe { libc::kill(-pid, sig) };
            then(pid);
            false
        });
    }

    /// Async-signal-safe.
    pub(super) fn terminate_all() {
        signal_all(libc::SIGTERM, |_| {});
    }

    /// Every tracked group, unregistered from the child guard (whose pipe closes at the exec
    /// and would otherwise end them) but left running and tracked.
    pub(super) fn hand_over() -> Vec<i32> {
        let mut groups = Vec::new();
        each_held(|pid| {
            crate::childguard::remove(pid);
            groups.push(pid);
            true
        });
        groups
    }

    /// Registers again with the child guard each of `groups` ([`hand_over`]) still tracked.
    pub(super) fn take_back(groups: &[i32]) {
        each_held(|pid| {
            if groups.contains(&pid) {
                crate::childguard::add(pid);
            }
            true
        });
    }

    extern "C" fn at_exit() {
        terminate_all();
    }

    extern "C" fn on_signal(sig: i32, info: *mut libc::siginfo_t, _: *mut libc::c_void) {
        if matches!(sig, libc::SIGTERM | libc::SIGINT | libc::SIGHUP) {
            // SAFETY: with SA_SIGINFO the kernel passes a valid siginfo_t (or null).
            let sender = unsafe { super::signal_sender(info) };
            super::log_ending_signal(sig, Some(sender), None);
        }
        terminate_all();
        if let Some((_, old)) = OLD.get().and_then(|o| o.iter().find(|(s, _)| *s == sig)) {
            // SAFETY: `old` is the disposition sigaction(2) reported for `sig`; sigaction is
            // async-signal-safe.
            unsafe { libc::sigaction(sig, old, std::ptr::null_mut()) };
        }
        // Pending until this handler returns; a hardware fault also re-executes and faults again.
        // SAFETY: raise(3) is async-signal-safe.
        unsafe { libc::raise(sig) };
    }

    pub(super) fn install() {
        static INSTALL: std::sync::Once = std::sync::Once::new();
        INSTALL.call_once(|| {
            // SAFETY: `at_exit` is an `extern "C" fn()` touching only the static `PIDS`, valid
            // whenever exit(3) runs it.
            unsafe { libc::atexit(at_exit) };
            // Recorded before any handler is installed: a handler that finds no previous
            // disposition to restore would re-raise into itself forever.
            let old = OLD.get_or_init(|| {
                FATAL
                    .into_iter()
                    .filter_map(|sig| {
                        // SAFETY: sigaction is integers, a handler address and a sigset_t, for
                        // which all zeros is valid.
                        let mut prev: libc::sigaction = unsafe { std::mem::zeroed() };
                        // SAFETY: a null new action only reads the current one into `prev`.
                        let r = unsafe { libc::sigaction(sig, std::ptr::null(), &mut prev) };
                        (r == 0 && prev.sa_sigaction != libc::SIG_IGN).then_some((sig, prev))
                    })
                    .collect()
            });
            for &(sig, _) in old {
                // SAFETY: as for `prev`.
                let mut act: libc::sigaction = unsafe { std::mem::zeroed() };
                act.sa_sigaction = on_signal as *const () as usize;
                act.sa_flags = libc::SA_SIGINFO | libc::SA_ONSTACK;
                // SAFETY: `act.sa_mask` is a live sigset_t for sigemptyset(3) to initialize.
                unsafe { libc::sigemptyset(&mut act.sa_mask) };
                // SAFETY: `act` is fully initialized; with SA_SIGINFO its handler `on_signal` has
                // the three-argument signature and does only async-signal-safe work.
                unsafe { libc::sigaction(sig, &act, std::ptr::null_mut()) };
            }
        });
    }
}

/// Where execvp would find `prog` (PATH from `env` first), as an explicit path: std spawns an
/// explicit path with posix_spawn (no fork of the shell's address space) even when `env`
/// overrides PATH, and a missing program is QProcess::FailedToStart before anything starts.
pub(crate) fn resolve(prog: &str, env: Option<&Value>) -> Option<std::path::PathBuf> {
    if prog.contains('/') {
        let p = std::path::PathBuf::from(prog);
        return crate::osutil::is_executable(&p).then_some(p);
    }
    let path = match env {
        Some(Value::Obj(o)) => o.borrow().props.get("PATH").map(|v| v.to_js_string()),
        _ => None,
    }
    // This process's PATH, the config's pragmas applied, not the user's: QProcess looks the
    // program up itself (QStandardPaths::findExecutable) before it starts it.
    .or_else(|| std::env::var("PATH").ok())
    .unwrap_or_else(|| "/usr/local/bin:/usr/bin:/bin".into());
    // QStandardPaths::findExecutable skips empty PATH entries.
    std::env::split_paths(&path)
        .filter(|d| !d.as_os_str().is_empty())
        .map(|d| d.join(prog))
        .find(|p| crate::osutil::is_executable(p))
}

/// The path this process was started as, links not resolved (`$PREFIX/bin/impulse-shell`, a link to
/// the current release), fixed by the first call: reload and the shell's own windows run it,
/// so they reach the release installed since, and still start once the running binary's
/// release was removed. It is `argv[0]`. A path is used if it is an executable file: this
/// binary, or a launcher that passed its own path on (`exec -a "$0"`, as the script that
/// tools/install.sh leaves under that name while it switches releases does). A bare name is
/// used only if PATH leads from it to this binary: the shell's own windows and helpers are
/// started under names that are not theirs (`qs`, `ri-pam`). Otherwise, and once the path is
/// gone, the binary's own path, then `/proc/self/exe`. A relative path (`bin/impulse-shell`, or the
/// name found through a relative PATH entry) is made absolute at that first call, from the
/// directory the process was started in.
pub fn launch_path() -> std::path::PathBuf {
    static STARTED: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    let started = STARTED.get_or_init(|| {
        let arg0 = std::env::args().next()?;
        let started_in = std::env::current_dir().ok()?;
        if arg0.contains('/') {
            let p = started_in.join(arg0);
            return crate::osutil::is_executable(&p).then_some(p);
        }
        let p = started_in.join(resolve(&arg0, None)?);
        (std::fs::canonicalize(&p).ok()? == std::env::current_exe().ok()?).then_some(p)
    });
    started
        .clone()
        .filter(|p| p.is_file())
        .or_else(|| std::env::current_exe().ok().filter(|p| p.is_file()))
        .unwrap_or_else(|| "/proc/self/exe".into())
}

/// What `environment` and `clearEnvironment` do to each variable, as Quickshell's
/// `setupProcessEnvironment` (src/io/processcore.cpp): the name with its value, `None` to
/// remove it. Without `clear` the variables are changes to the user's environment
/// ([`workpool::userenv`], not this process's: the config's pragmas reach the shell alone)
/// and `null` removes one; with it they are the only ones the program sees, and `null` takes
/// the user's value. `undefined` (an invalid QVariant) changes nothing.
fn env_changes(env: Option<&Value>, clear: bool) -> Vec<(String, Option<std::ffi::OsString>)> {
    let Some(Value::Obj(o)) = env else {
        return Vec::new();
    };
    o.borrow()
        .props
        .iter()
        .filter_map(|(k, v)| match v {
            Value::Undefined => None,
            Value::Null if clear => workpool::userenv::var(&**k).map(|v| (k.to_string(), Some(v))),
            Value::Null => Some((k.to_string(), None)),
            v => Some((k.to_string(), Some(v.to_js_string().into()))),
        })
        .collect()
}

/// A Process's `environment`, `clearEnvironment` and `workingDirectory` on `c`.
fn command_env(
    c: &mut std::process::Command,
    env: Option<&Value>,
    clear: bool,
    cwd: Option<String>,
) {
    if clear {
        c.env_clear();
    } else {
        workpool::userenv::apply(c);
    }
    for (k, v) in env_changes(env, clear) {
        match v {
            Some(v) => c.env(k, v),
            None => c.env_remove(k),
        };
    }
    if let Some(d) = cwd.filter(|d| !d.is_empty()) {
        c.current_dir(d);
    }
}

/// The whole environment of a program started detached with `environment` and
/// `clearEnvironment` ([`env_changes`]).
fn detached_env(env: Option<&Value>, clear: bool) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    let mut vars: Vec<(std::ffi::OsString, std::ffi::OsString)> = if clear {
        Vec::new()
    } else {
        workpool::userenv::vars()
    };
    for (k, v) in env_changes(env, clear) {
        let k = std::ffi::OsString::from(k);
        vars.retain(|(name, _)| *name != k);
        if let Some(v) = v {
            vars.push((k, v));
        }
    }
    vars
}

static RELOAD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether `Rustshell.reload()` was called since the last check.
pub fn take_reload() -> bool {
    RELOAD.swap(false, std::sync::atomic::Ordering::Relaxed)
}

/// Names the `Process` groups a reload hands to the next image ([`hand_over_children`]).
pub const HANDED_OVER_ENV: &str = "RI_HANDED_OVER";

/// A reload's exec: every running `Process` group (and the programs in-process jobs and
/// [`crate::child::run`] started), unregistered from the child guard and left running. The
/// next image kills them ([`end_handed_over`]), as their destruction does on a Quickshell
/// reload; a failed exec hands them back ([`take_back_children`]) untouched. A group started
/// after this stays guarded.
pub fn hand_over_children() -> Vec<i32> {
    children::hand_over()
}

/// After a failed exec: guards again the groups [`hand_over_children`] returned.
pub fn take_back_children(groups: &[i32]) {
    children::take_back(groups);
}

/// The re-executed shell: SIGKILL each group in `groups` (the previous image's
/// [`HANDED_OVER_ENV`]) whose leader is among `inherited` ([`inherited_children`]), without
/// waiting for it. An unreaped leader keeps its id, so its group is the one handed over; one
/// that exited and was reaped before the exec is skipped. Call before [`reap_inherited`].
pub fn end_handed_over(groups: &[i32], inherited: &[i32]) {
    for &pgid in groups.iter().filter(|p| **p > 0 && inherited.contains(p)) {
        // SAFETY: kill(2) takes plain integers.
        unsafe { libc::kill(-pgid, libc::SIGKILL) };
    }
}

/// Installs the handlers that terminate the `Process` groups when the shell dies (exit, a
/// fatal signal). Call at startup, before any child exists.
pub fn install_shutdown_handlers() {
    children::install();
}

/// Where a signal came from, as its siginfo_t tells.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SignalSender {
    /// A process sent it (kill(2), sigqueue(3), tgkill(2)): its pid.
    Process(i32),
    /// The kernel raised it itself (a terminal's hangup, a timer).
    Kernel,
}

/// Who sent the signal `info` describes. Async-signal-safe.
///
/// # Safety
/// `info` is null or what the kernel passed a handler installed with `SA_SIGINFO`.
pub unsafe fn signal_sender(info: *const libc::siginfo_t) -> SignalSender {
    // SAFETY: not null, so the kernel's siginfo_t.
    match unsafe { info.as_ref() } {
        // SAFETY: `si_pid` is set in a signal a process sent, which `si_code <= 0` says
        // (sigaction(2)).
        Some(info) if info.si_code <= 0 => SignalSender::Process(unsafe { info.si_pid() }),
        _ => SignalSender::Kernel,
    }
}

/// Says in the log (stderr) that the termination signal `sig` ends the shell and who sent it
/// (`None`: not known), with the sender's name when the caller has it:
/// `impulse-shell: ending on signal 15 (SIGTERM) sent by pid 1234 (killall)`. Without the line a shell
/// that `killall` or a session manager ended and one the kernel killed (SIGKILL, which no
/// process can report) leave the same log, one that just stops. One write(2) from a buffer on
/// the stack: async-signal-safe, for the handlers of a shell that is still starting.
pub fn log_ending_signal(sig: i32, sender: Option<SignalSender>, sender_name: Option<&[u8]>) {
    struct Line([u8; 128], usize);
    impl Line {
        fn text(&mut self, t: &[u8]) {
            // The last byte stays free for the newline.
            let n = t.len().min(self.0.len() - 1 - self.1);
            self.0[self.1..self.1 + n].copy_from_slice(&t[..n]);
            self.1 += n;
        }
        fn number(&mut self, n: i32) {
            let mut digits = [0u8; 10];
            let (mut at, mut n) = (digits.len(), n.unsigned_abs());
            loop {
                at -= 1;
                digits[at] = b'0' + (n % 10) as u8;
                n /= 10;
                if n == 0 {
                    break;
                }
            }
            self.text(&digits[at..]);
        }
    }
    let mut l = Line([0; 128], 0);
    l.text(b"impulse-shell: ending on signal ");
    l.number(sig);
    l.text(match sig {
        libc::SIGTERM => b" (SIGTERM)",
        libc::SIGINT => b" (SIGINT)",
        libc::SIGHUP => b" (SIGHUP)",
        _ => b"",
    });
    match sender {
        Some(SignalSender::Process(pid)) => {
            l.text(b" sent by pid ");
            l.number(pid);
        }
        Some(SignalSender::Kernel) => l.text(b" sent by the kernel"),
        None => {}
    }
    if let Some(name) = sender_name {
        l.text(b" (");
        l.text(name);
        l.text(b")");
    }
    l.0[l.1] = b'\n';
    // SAFETY: the buffer holds `l.1 + 1` initialized bytes.
    unsafe { libc::write(libc::STDERR_FILENO, l.0.as_ptr().cast(), l.1 + 1) };
}

/// SIGTERM every running `Process` group, as the shell's exit does.
pub fn terminate_children() {
    children::terminate_all();
}

/// The children a re-executed shell inherited from its previous image: the `Process` leaders
/// it handed over ([`end_handed_over`] kills them), the child guard (it exits once `exec` closes its
/// pipe) and detached programs still running. Call at startup, before this image starts any
/// child of its own, and hand the list to [`reap_inherited`].
pub fn inherited_children() -> Vec<i32> {
    workpool::children()
}

/// Reaps each of `pids` ([`inherited_children`]) when it exits; nothing else of this image
/// waits for them. Their ids stay theirs until reaped, so the list may be taken earlier.
pub fn reap_inherited(pids: Vec<i32>) {
    for pid in pids {
        workpool::adopt(pid);
    }
}

/// Writes every edit of `Config.options` still waiting in Config.qml's delayed save; called
/// when the shell exits, which would otherwise drop it.
pub fn save_pending(w: &mut World) {
    save_pending_config(w);
}

/// Writes an edit of `Config.options` still waiting in Config.qml's delayed save, so the
/// command about to start reads it from config.json. The original sets an option and starts the command
/// that applies it in one handler (`palette.type = v; execDetached([switchwall.sh --noswitch])`);
/// the native switchwall reads the file at once, before that save, and applied the previous
/// scheme until the next click.
pub(crate) fn save_pending_config(w: &mut World) {
    if w.adapter_unsaved.is_empty() {
        return;
    }
    let Some(&cfg) = w.singletons.get("Config") else {
        return;
    };
    let path = match w.read(cfg, "filePath") {
        Some(Value::Str(s)) => s.to_string(),
        _ => return,
    };
    let path = crate::qurl::trim_file_protocol(&path).to_string();
    let mut fvs: Vec<usize> = w.adapter_unsaved.iter().copied().collect();
    fvs.sort_unstable();
    for fv in fvs {
        if file_path(w, fv) == path {
            write_adapter(w, fv);
        }
    }
}

/// Generation and duration of the last crossfade request (`Crossfade.request`).
static CROSSFADE: std::sync::Mutex<(u64, std::time::Duration)> =
    std::sync::Mutex::new((0, std::time::Duration::ZERO));

/// The windows the same loop turn's changes redraw fade their next frame in over their previous
/// one. Each process asks for its own fades where its colors change (MaterialThemeLoader, right
/// before it applies them), so a theme switch fades every impulse-shell window exactly when its colors
/// change.
fn bump_crossfade(ms: u64) {
    let mut c = CROSSFADE.lock().unwrap();
    *c = (
        c.0 + 1,
        std::time::Duration::from_millis(ms.clamp(50, 2000)),
    );
}

/// `Crossfade.request(ms)`; a duration that is not a number is 300 ms.
fn crossfade_request(_: &mut Interp, _: &Value, args: &[Value]) -> R<Value> {
    let ms = args.first().map(Value::to_number).unwrap_or(f64::NAN);
    bump_crossfade(if ms.is_finite() {
        ms.max(0.0) as u64
    } else {
        300
    });
    Ok(Value::Undefined)
}

/// The `Crossfade` singleton of module `Impulse` (`Impulse/crossfade.qmltypes`).
pub static CROSSFADE_TYPE: crate::impulse::Type = crate::impulse::Type {
    name: "Crossfade",
    methods: &[("request", crossfade_request)],
    init: None,
};

pub fn crossfade_gen() -> u64 {
    CROSSFADE.lock().unwrap().0
}

/// One window's view of the crossfade requests. A request comes with its change (the colors
/// applied right after it): the windows that redraw in the same loop turn take it for their
/// next frame and the others drop it, so nothing stays armed for a later, unrelated change and
/// nothing expires on a timer. A frame showing what the window already shows never fades.
#[derive(Debug)]
pub struct FadeArm {
    seen: u64,
    armed: Option<std::time::Duration>,
}

impl FadeArm {
    /// Takes no request made before it.
    pub fn new() -> Self {
        FadeArm {
            seen: crossfade_gen(),
            armed: None,
        }
    }

    /// Notes the requests made since the last call, once a loop turn's changes are known:
    /// `redraws` is whether the window draws for them.
    pub fn observe(&mut self, redraws: bool) {
        let (count, d) = *CROSSFADE.lock().unwrap();
        if count == self.seen {
            return;
        }
        if redraws {
            self.armed = self.armed.max(Some(d));
        }
        self.seen = count;
    }

    /// The crossfade the frame being drawn starts. `differs` says whether that frame differs
    /// from the window's previous one (None without one to fade from); it compares whole
    /// frames, so it runs only when a request is armed.
    pub fn take(&mut self, differs: impl FnOnce() -> Option<bool>) -> Option<std::time::Duration> {
        let d = self.armed.take()?;
        (differs() == Some(true)).then_some(d)
    }
}

impl Default for FadeArm {
    fn default() -> Self {
        Self::new()
    }
}

/// `Rustshell.execDetached(cmd)`.
pub fn exec_detached(w: &mut World, v: &Value) {
    save_pending_config(w);
    spawn_detached(v);
}

/// `Process.startDetached()`: the Process's `command` with its `environment`,
/// `clearEnvironment` and `workingDirectory`, as Quickshell's `Process::startDetached`.
pub(crate) fn start_detached(w: &mut World, p: usize) {
    save_pending_config(w);
    let argv = str_list(&w.read(p, "command").unwrap_or_default());
    let env = w.read(p, "environment");
    let clear = w.read(p, "clearEnvironment").is_some_and(|v| v.truthy());
    let cwd = w.read(p, "workingDirectory").map(|d| d.to_js_string());
    detach_context("Process.startDetached", argv, env.as_ref(), clear, cwd);
}

/// Fire and forget: the child starts in its own session and the reaper waits for it
/// ([`detach`]), so it leaves no zombie; one that outlives a `Rustshell.reload()` re-exec is
/// waited for by the new image ([`workpool::adopt`]). `v` is the command or a context object
/// (`command`, `environment`, `clearEnvironment`, `workingDirectory`).
pub fn spawn_detached(v: &Value) {
    let (argv, env, clear, cwd) = match v {
        Value::Obj(o) if o.borrow().array.is_none() => {
            let o = o.borrow();
            (
                str_list(&o.props.get("command").cloned().unwrap_or_default()),
                o.props.get("environment").cloned(),
                o.props.get("clearEnvironment").is_some_and(|v| v.truthy()),
                o.props.get("workingDirectory").map(|d| d.to_js_string()),
            )
        }
        v => (str_list(v), None, false, None),
    };
    detach_context("Rustshell.execDetached", argv, env.as_ref(), clear, cwd);
}

/// Starts `argv` detached for the QML call `what`, as `RustshellGlobal::execDetached` starts
/// its `ProcessContext`.
fn detach_context(
    what: &'static str,
    argv: Vec<String>,
    env: Option<&Value>,
    clear: bool,
    cwd: Option<String>,
) {
    if argv.is_empty() {
        elog::eprintln!("{what}: cannot start process as command is empty");
        return;
    }
    if hooked_spawn(&argv).is_some() {
        return;
    }
    detach(
        what,
        crate::detached::Launch {
            exe: resolve(&argv[0], env),
            argv,
            env: detached_env(env, clear),
            cwd,
        },
    );
}

/// `ShellControl.openWindow(file)`: one of the shell's app windows (settings.qml, welcome.qml,
/// killDialog.qml), run by this binary as `-p FILE` and keyed by the file's canonical path.
/// Config changes not saved yet are written first, for the window's process to read.
pub(crate) fn open_own_window(w: &mut World, file: &str) {
    save_pending_config(w);
    if hooked_call(crate::shellcontrol::NAME, "openWindow", &[file]).is_some() {
        return;
    }
    let Ok(key) = std::fs::canonicalize(file) else {
        elog::eprintln!("ShellControl.openWindow: no file {file}");
        return;
    };
    let exe = launch_path();
    let launch = crate::detached::Launch {
        argv: vec![exe.to_string_lossy().into_owned(), "-p".into(), file.into()],
        exe: Some(exe),
        env: workpool::userenv::vars(),
        cwd: None,
    };
    open_app_window(
        key,
        move || {
            launch
                .start("ShellControl.openWindow")
                .map(|pid| pid as u32)
        },
        focus_pid,
    );
}

/// App windows this shell started and that still run (pid 0: being spawned).
static APP_WINDOWS: std::sync::Mutex<Vec<(std::path::PathBuf, u32)>> =
    std::sync::Mutex::new(Vec::new());
/// Notified after each change of [`APP_WINDOWS`] (a pid stored, an entry forgotten).
static APP_WINDOWS_CHANGED: std::sync::Condvar = std::sync::Condvar::new();

/// Blocks until `done` holds for [`APP_WINDOWS`], woken by its changes.
#[cfg(test)]
fn wait_app_windows(done: impl Fn(&[(std::path::PathBuf, u32)]) -> bool) {
    let apps = APP_WINDOWS.lock().unwrap();
    drop(APP_WINDOWS_CHANGED.wait_while(apps, |a| !done(a)).unwrap());
}

/// One process and window per app window file: asked for again while that window runs, it is
/// focused instead (as the dock or a launcher does for a running app); once it exits, the next
/// call opens it again.
fn open_app_window(
    key: std::path::PathBuf,
    spawn: impl FnOnce() -> Option<u32> + Send + 'static,
    focus: impl FnOnce(u32) + Send + 'static,
) {
    let mut apps = APP_WINDOWS.lock().unwrap();
    if let Some(&(_, pid)) = apps.iter().find(|(k, _)| *k == key) {
        drop(apps);
        if pid != 0 {
            crate::hypr::submit(move || focus(pid));
        }
        return;
    }
    apps.push((key.clone(), 0));
    drop(apps);
    let run = move || {
        workpool::run(move || {
            let forget = |key: &std::path::Path| {
                APP_WINDOWS.lock().unwrap().retain(|(k, _)| k != key);
                APP_WINDOWS_CHANGED.notify_all();
            };
            let Some(pid) = spawn() else {
                return forget(&key);
            };
            if let Some(e) = APP_WINDOWS
                .lock()
                .unwrap()
                .iter_mut()
                .find(|(k, _)| *k == key)
            {
                e.1 = pid;
            }
            APP_WINDOWS_CHANGED.notify_all();
            workpool::adopt_then(pid as i32, move |_| forget(&key));
        })
    };
    let run = HELD_APP_WINDOWS.with_borrow_mut(|held| match held {
        Some(q) => {
            q.push(Box::new(run));
            None
        }
        None => Some(run),
    });
    if let Some(run) = run {
        run();
    }
}

type HeldSpawn = Box<dyn FnOnce()>;

thread_local! {
    /// App windows asked for while the shell's startup panels have not reserved their exclusive
    /// zones yet (None: not holding). welcome.qml opens at first run from shell.qml's load, before
    /// the bar is on screen; Hyprland would center it on the whole output instead of the area left
    /// under the bar (`qs -c impulse-shell` starts it through bash and a second qs, which takes longer).
    /// Only the main thread asks for app windows (QML handlers) and releases them.
    static HELD_APP_WINDOWS: std::cell::RefCell<Option<Vec<HeldSpawn>>> =
        const { std::cell::RefCell::new(None) };
}

/// Queue app windows until [`release_app_windows`].
pub fn hold_app_windows() {
    HELD_APP_WINDOWS.with_borrow_mut(|h| {
        h.get_or_insert_with(Vec::new);
    });
}

pub fn app_windows_held() -> bool {
    HELD_APP_WINDOWS.with_borrow(Option::is_some)
}

/// Open the app windows queued since [`hold_app_windows`]; later ones open right away.
pub fn release_app_windows() {
    let held = HELD_APP_WINDOWS.take();
    for run in held.into_iter().flatten() {
        run();
    }
}

/// Focus the Hyprland window of process `pid` (and its workspace).
fn focus_pid(pid: u32) {
    if let Some(d) = crate::hypr::socket_dir() {
        crate::hypr::request(
            &d,
            &format!("/dispatch hl.dsp.focus({{window = \"pid:{pid}\"}})"),
        );
    }
}

/// `argv` as a detached program with the user's environment, as `execDetached`. Under a
/// [`SpawnHook`] the hook gets `argv` and nothing starts.
pub(crate) fn detach_program(argv: &[String]) {
    if hooked_spawn(argv).is_none() {
        detach(
            "detached program",
            crate::detached::Launch::inherit(argv, None),
        );
    }
}

/// [`detach`] starts handed to the pool whose `spawn()` has not returned yet.
static DETACHING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Signalled each time a detached start returns ([`flush_detached`] waits on it).
static DETACHED: (std::sync::Mutex<()>, std::sync::Condvar) =
    (std::sync::Mutex::new(()), std::sync::Condvar::new());

/// Returns once every program `execDetached` (or `Process.startDetached`) asked for has been
/// started: the shell's exit calls this, so a program a handler started right before
/// `Qt.quit()`, or `Component.onDestruction` started at the exit, runs as in Quickshell, whose
/// `QProcess::startDetached` returns with the program already started.
pub fn flush_detached() {
    let mut all = DETACHED.0.lock().unwrap_or_else(|e| e.into_inner());
    while detaching() != 0 {
        all = DETACHED.1.wait(all).unwrap_or_else(|e| e.into_inner());
    }
}

/// Detached starts not spawned yet. With this process's live children it accounts for every
/// process `execDetached` asked for: from the call until the child exits (tests).
pub fn detaching() -> usize {
    DETACHING.load(std::sync::atomic::Ordering::Acquire)
}

/// Starts `launch` on a pool thread (the caller does not wait for the program to start) and
/// reaps it when it exits. A failed start is logged for `what` with the command line.
pub(crate) fn detach(what: &'static str, launch: crate::detached::Launch) {
    struct Started;
    impl Drop for Started {
        fn drop(&mut self) {
            let _all = DETACHED.0.lock().unwrap_or_else(|e| e.into_inner());
            DETACHING.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
            DETACHED.1.notify_all();
        }
    }
    DETACHING.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    let started = Started;
    workpool::run(move || {
        let _started = started;
        if let Some(pid) = launch.start(what) {
            workpool::adopt(pid);
        }
    });
}

/// Starts Process `p` with the command line `argv`, its `command` when there is none.
fn spawn(w: &mut World, p: usize, argv: Option<Value>) -> Option<Proc> {
    // Like execDetached: the command reads options set right before it from config.json.
    save_pending_config(w);
    let cmd = argv.unwrap_or_else(|| w.read(p, "command").unwrap_or_default());
    if crate::impulse::has_job(w, p) {
        // Under a spawn hook (tests, aotcheck) the job does not run: it would reach the
        // network or the disk. The hook answers for it as for a command.
        if sandboxed() {
            let out = hooked_spawn(&crate::impulse::hook_argv(w, p)).flatten();
            let mut queued = std::collections::VecDeque::new();
            if let Some(o) = &out {
                queued.push_back(ProcEv::Out(o.clone()));
            }
            queued.push_back(ProcEv::Exit(if out.is_some() { 0 } else { 1 }));
            return Some(Proc {
                pid: 0,
                stop: None,
                queued,
                job: None,
                io: None,
                out: String::new(),
                err: String::new(),
                restart: false,
                stdin: None,
                after_frame: None,
            });
        }
        let job = crate::impulse::job(w, p)?;
        let (tx, rx) = w.waker.handle(crate::svc::Svc::Process).channel();
        let stop = std::sync::Arc::new(crate::svc::Stop::default());
        crate::jobs::start(job, tx, stop.clone());
        return Some(Proc {
            pid: 0,
            stop: Some(stop),
            queued: Default::default(),
            job: Some(rx),
            io: None,
            out: String::new(),
            err: String::new(),
            restart: false,
            stdin: None,
            after_frame: None,
        });
    }
    let argv = str_list(&cmd);
    if argv.is_empty() {
        return None;
    }
    if let Some(out) = hooked_spawn(&argv) {
        let mut queued = std::collections::VecDeque::new();
        if let Some(o) = &out {
            queued.push_back(ProcEv::Out(o.clone()));
        }
        queued.push_back(ProcEv::Exit(if out.is_some() { 0 } else { 1 }));
        return Some(Proc {
            pid: 0,
            stop: None,
            queued,
            job: None,
            io: None,
            out: String::new(),
            err: String::new(),
            restart: false,
            stdin: None,
            after_frame: None,
        });
    }
    let env = w.read(p, "environment");
    let cwd = w.read(p, "workingDirectory").map(|d| d.to_js_string());
    let Some(exe) = resolve(&argv[0], env.as_ref()) else {
        elog::eprintln!("Process {argv:?}: failed to start, program not found");
        return None;
    };
    // No pre_exec hook (PR_SET_PDEATHSIG) and no setpriv wrapper: std posix_spawn()s the
    // child, where a fork() of the shell would copy its page tables on the frame thread.
    let mut c = std::process::Command::new(exe);
    {
        use std::os::unix::process::CommandExt;
        c.arg0(&argv[0]).process_group(0);
    }
    let stdin_on = w.read(p, "stdinEnabled").is_some_and(|v| v.truthy());
    let pending = w.stdin_pending.remove(&p);
    c.args(&argv[1..]).stdin(if stdin_on {
        std::process::Stdio::piped()
    } else {
        std::process::Stdio::null()
    });
    let clear = w.read(p, "clearEnvironment").is_some_and(|v| v.truthy());
    command_env(&mut c, env.as_ref(), clear, cwd);
    c.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let ep = proc_ep(w)?;
    let mut ch = match c.spawn() {
        Ok(ch) => ch,
        Err(e) => {
            elog::eprintln!("Process {argv:?}: {e}");
            return None;
        }
    };
    let pid = ch.id() as i32;
    let pidfd = match workpool::pidfd_open(pid) {
        Ok(fd) => fd,
        Err(e) => {
            // Unreaped, so the group id is still the child's.
            // SAFETY: kill(2) takes plain integers and touches no memory of this process.
            unsafe { libc::kill(-pid, libc::SIGKILL) };
            workpool::reap(ch);
            elog::eprintln!("Process {argv:?}: pidfd_open: {e}");
            return None;
        }
    };
    children::add(pid);
    crate::childguard::add(pid);
    let key = (p as u64) << 2;
    let pipes: [Option<std::os::fd::OwnedFd>; 2] = [
        ch.stdout.take().map(Into::into),
        ch.stderr.take().map(Into::into),
    ];
    // The exit is watched from the start: the Process ends when its child dies, not when
    // every descendant holding its pipes lets go of them.
    let watched = {
        use std::os::fd::AsRawFd;
        ep_add(ep, pidfd.as_raw_fd(), key | 2)
            && pipes.iter().enumerate().all(|(k, f)| {
                f.as_ref().is_some_and(|f| {
                    set_nonblocking(f.as_raw_fd());
                    ep_add(ep, f.as_raw_fd(), key | k as u64)
                })
            })
    };
    if !watched {
        // The epoll set refused a descriptor (ENOMEM, max_user_watches): nobody would read the
        // pipes or see the exit, so the child does not run. Closing the fds unregisters them.
        elog::eprintln!(
            "Process {argv:?}: cannot watch it: {}",
            std::io::Error::last_os_error()
        );
        // Unreaped, so the group id is still the child's.
        // SAFETY: kill(2) takes plain integers and touches no memory of this process.
        unsafe { libc::kill(-pid, libc::SIGKILL) };
        children::remove(pid);
        crate::childguard::remove(pid);
        workpool::reap(ch);
        return None;
    }
    let mut stdin = ch.stdin.take().map(|pipe| Stdin::new(pipe, ep, key));
    if let Some(s) = stdin.as_mut() {
        s.buf.extend(pending.unwrap_or_default());
        if !s.flush() {
            stdin = None;
        }
    }
    Some(Proc {
        pid,
        stop: None,
        queued: Default::default(),
        job: None,
        io: Some(ProcIo {
            pidfd: Some(pidfd),
            ch: Some(ch),
            pipes,
            pend: Default::default(),
        }),
        out: String::new(),
        err: String::new(),
        restart: false,
        stdin,
        after_frame: None,
    })
}

/// Process work without a fresh wakeup: a readable pipe/pidfd, events queued at spawn (hooked
/// spawns), or job output held for a frame (`after_frame`), which the frame's ping releases.
/// A host that watches [`Runtime::process_fd`] reports its readiness instead
/// ([`Runtime::process_fd_ready`]), so the set is not polled again here.
fn procs_pending(w: &World) -> bool {
    !w.proc_ep_watched && proc_ready(w)
        || w.procs
            .values()
            .any(|pr| !pr.queued.is_empty() || pr.job.is_some() && pr.after_frame.is_some())
}

/// Some Process pipe or pidfd is readable (the epoll set is level-triggered; nothing is consumed).
fn proc_ready(w: &World) -> bool {
    use std::os::fd::AsRawFd;
    w.proc_ep.as_ref().is_some_and(|ep| {
        let mut p = libc::pollfd {
            fd: ep.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: `p` is one live pollfd for the duration of the call.
        (unsafe { libc::poll(&mut p, 1, 0) }) > 0
    })
}

/// Process.write(data), as Quickshell's: goes to the child's stdin while it has one, is buffered
/// from `running = true` until the spawn at component completion (QProcess buffers writes while
/// starting), and is dropped otherwise.
pub(crate) fn write_stdin(w: &mut World, p: usize, data: &str) {
    if let Some(pr) = w.procs.get_mut(&p) {
        if let Some(s) = pr.stdin.as_mut() {
            s.buf.extend(data.as_bytes());
            pr.flush_stdin();
        }
    } else if w.read(p, "running").is_some_and(|v| v.truthy()) {
        w.stdin_pending
            .entry(p)
            .or_default()
            .extend_from_slice(data.as_bytes());
    }
}

/// The World's epoll set, holding every Process pipe and pidfd and the file watcher's inotify
/// fd: the main loop polls this one fd.
pub(crate) fn proc_ep(w: &mut World) -> Option<i32> {
    use std::os::fd::{AsRawFd, FromRawFd};
    if w.proc_ep.is_none() {
        // SAFETY: epoll_create1(2) takes a plain flag.
        let fd = unsafe { libc::epoll_create1(libc::EPOLL_CLOEXEC) };
        if fd < 0 {
            elog::eprintln!("Process: epoll: {}", std::io::Error::last_os_error());
            return None;
        }
        // SAFETY: `fd` is the epoll fd just created, open and owned by nothing else.
        w.proc_ep = Some(unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) });
    }
    w.proc_ep.as_ref().map(|f| f.as_raw_fd())
}

fn has_type(w: &World, i: usize, t: &str) -> bool {
    w.items[i].types.iter().any(|x| x == t)
}

pub fn emit(w: &mut World, i: usize, sig: &str, params: &[&str], args: Vec<Value>) {
    w.signals
        .entry(i)
        .or_default()
        .entry(sig.to_string())
        .or_insert_with(|| {
            params
                .iter()
                .map(|s| crate::runtime::SigParam::untyped(s))
                .collect()
        });
    w.emits.push((i, sig.to_string(), args));
}

/// Feed process output to its `stdout`/`stderr` parser (SplitParser emits per segment).
fn feed(w: &mut World, p: usize, stream: &str, buf: &mut String, end: bool) {
    let Some(Value::Item(c)) = w.read(p, stream) else {
        return;
    };
    if has_type(w, c, "SplitParser") {
        let marker = match w.read(c, "splitMarker") {
            Some(Value::Str(s)) if !s.is_empty() => s.to_string(),
            _ => "\n".to_string(),
        };
        while let Some(k) = buf.find(&marker) {
            let seg = buf[..k].to_string();
            buf.drain(..k + marker.len());
            emit(w, c, "read", &["data"], vec![Value::str(&seg)]);
        }
        if end && !buf.is_empty() {
            let seg = std::mem::take(buf);
            emit(w, c, "read", &["data"], vec![Value::str(&seg)]);
        }
    } else if has_type(w, c, "StdioCollector") && end {
        let text = std::mem::take(buf);
        w.write(c, "text", Value::str(&text));
        w.write(c, "data", Value::str(&text));
        // Queued with the Process signals so `streamFinished` precedes `exited` as in Quickshell.
        emit(w, c, "streamFinished", &[], vec![]);
    }
}

/// Bring a `SystemClock` up to the wall clock at its precision; true when it changed.
pub(crate) fn clock_tick(world: &RefCell<World>, it: &mut Interp, c: usize) -> bool {
    let now = (crate::builtins::now_ms() / 1000.0).floor() as i64;
    // SystemClock only ticks at its precision (Hours = 1, Minutes = 2, Seconds = 3).
    let precision = world
        .borrow_mut()
        .read(c, "precision")
        .map_or(3.0, |v| v.to_number());
    let step = match precision as i64 {
        1 => 3600,
        2 => 60,
        _ => 1,
    };
    if world.borrow().clock_holds(c, step, now) {
        return false;
    }
    // What local clocks read, in seconds as if it were UTC.
    let secs = now + i64::from(crate::tz::local_ahead(now));
    let from = now - secs.rem_euclid(step);
    world.borrow_mut().clock_shows(c, step, from);
    let ms = (from * 1000) as f64;
    let prev = world
        .borrow_mut()
        .read(c, "date")
        .and_then(|v| crate::builtins::date_ms(&v));
    if prev == Some(ms) {
        return false;
    }
    let d = crate::builtins::new_date(it, ms);
    let mut w = world.borrow_mut();
    w.write(c, "date", d);
    let hour = secs.rem_euclid(86400) / 3600;
    w.write(c, "hours", Value::Num(hour as f64));
    let sec = if step == 1 { secs.rem_euclid(60) } else { 0 };
    let min = match step {
        3600 => 0,
        _ => secs.rem_euclid(3600) / 60,
    };
    w.write(c, "minutes", Value::Num(min as f64));
    w.write(c, "seconds", Value::Num(sec as f64));
    true
}

/// Quickshell's SystemClock reads the time in its constructor: bindings and handlers of a
/// new object tree never see the 00:00 placeholder that the first poll would replace.
pub(crate) fn init_clocks(world: &RefCell<World>, it: &mut Interp, n0: usize) {
    let clocks: Vec<usize> = {
        let w = world.borrow();
        (n0..w.items.len())
            .filter(|&i| w.items[i].types.iter().any(|t| t == "SystemClock"))
            .collect()
    };
    for c in clocks {
        clock_tick(world, it, c);
    }
}

pub fn poll_processes(w: &mut World) -> bool {
    let mut changed = false;
    drain_processes(w, &mut changed);
    changed
}

/// `running` of an image type of the `Impulse` module was written (`crate::imgjobs`, its
/// `Element::running`): true starts a run unless one is going, false stops the one going. The
/// run is a [`Proc`], so the item's destruction stops it too and a frame-pacing host holds its
/// result for a frame (`Proc::after_frame`).
pub(crate) fn image_running(w: &mut World, p: usize, run: bool) {
    if !run {
        // The job was already stopped by `running_write`.
        return;
    }
    if w.procs.contains_key(&p) || w.dead.contains(&p) {
        return;
    }
    let pr = match crate::imgjobs::start(w, p) {
        // Sandboxed (aotcheck): no image is read, the run ends at the next poll with no result.
        Some(_) if sandboxed() => Some(Proc {
            pid: 0,
            stop: None,
            queued: [ProcEv::Exit(1)].into(),
            job: None,
            io: None,
            out: String::new(),
            err: String::new(),
            restart: false,
            stdin: None,
            after_frame: None,
        }),
        Some(job) => {
            let h = w.waker.handle(crate::svc::Svc::Process);
            let after_frame = w.frame_paced.then_some(w.frame_seq);
            let (rx, stop) = crate::imgjobs::spawn(w, p, job, &h);
            Some(Proc {
                pid: 0,
                stop: Some(stop),
                queued: Default::default(),
                job: Some(rx),
                io: None,
                out: String::new(),
                err: String::new(),
                restart: false,
                stdin: None,
                after_frame,
            })
        }
        None => None,
    };
    match pr {
        Some(pr) => {
            w.procs.insert(p, pr);
        }
        None => {
            w.write(p, "running", Value::Bool(false));
            crate::imgjobs::finished(w, p);
        }
    }
}

/// The queued events of image item `p`'s run: its output becomes the type's signals, its end
/// `finished`.
fn drain_image(w: &mut World, p: usize) {
    while let Some(ev) = w.procs.get_mut(&p).and_then(|pr| pr.queued.pop_front()) {
        match ev {
            ProcEv::Out(s) => {
                let mut b = std::mem::take(&mut w.procs.get_mut(&p).unwrap().out);
                b.push_str(&s);
                crate::imgjobs::output(w, p, &mut b, false);
                w.procs.get_mut(&p).unwrap().out = b;
            }
            ProcEv::File { done, total, path } => {
                crate::imgjobs::file_done(w, p, done, total, &path)
            }
            ProcEv::Exit(_) | ProcEv::Signaled(_) => {
                let mut pr = w.procs.remove(&p).unwrap();
                // The job ended by itself: its drop has nothing to stop.
                pr.stop = None;
                crate::imgjobs::output(w, p, &mut pr.out, true);
                w.write(p, "running", Value::Bool(false));
                crate::imgjobs::finished(w, p);
                if pr.restart {
                    w.write(p, "running", Value::Bool(true));
                }
                return;
            }
            _ => {}
        }
    }
}

/// Quickshell Process::setRunning / componentComplete: a Process starts the moment it is set
/// running (after its component completed), not at the next poll; `false` stops in-process
/// command implementations at once (a child is only terminated, see `running_write`).
pub(crate) fn running_changed(w: &mut World, p: usize, run: bool) {
    let argv = w.argv_pending.remove(&p);
    if crate::impulse::running(w, p, run) {
        return;
    }
    if !run {
        w.stdin_pending.remove(&p);
        if let Some(pr) = w.procs.remove(&p) {
            if pr.pid == 0 {
                terminate_job(w, p, pr);
            } else {
                pr.kill(libc::SIGTERM);
            }
        }
        return;
    }
    // Set running by `Component.onDestruction` while the shell ends: Quickshell's ~Process
    // kills what it started the moment its root is deleted, so nothing of it runs.
    if w.exiting
        || w.procs.contains_key(&p)
        || w.dead.contains(&p)
        || !w.items[p].types.iter().any(|t| t == "Process")
    {
        return;
    }
    match spawn(w, p, argv) {
        Some(pr) => {
            crate::weather::started(w, p);
            w.write(p, "processId", Value::Num(pr.pid as f64));
            w.procs.insert(p, pr);
            w.fire.push(crate::runtime::Fire::Handler(
                p,
                crate::runtime::Sym::new("onStarted"),
            ));
        }
        None => {
            w.write(p, "running", Value::Bool(false));
        }
    }
}

fn drain_processes(w: &mut World, changed: &mut bool) {
    if let Some(ep) = w.proc_ep.as_ref().map(std::os::fd::AsRawFd::as_raw_fd) {
        let mut evs = [libc::epoll_event { events: 0, u64: 0 }; 64];
        loop {
            // SAFETY: `evs` is a live array of 64 epoll_events for the kernel to fill.
            let n = unsafe { libc::epoll_wait(ep, evs.as_mut_ptr(), 64, 0) }.max(0) as usize;
            for e in evs[..n].iter().filter(|e| e.u64 != EP_FSWATCH) {
                let (p, k) = ((e.u64 >> 2) as usize, (e.u64 & 3) as usize);
                if let Some(pr) = w.procs.get_mut(&p) {
                    if k == 3 {
                        pr.flush_stdin();
                    } else if let Some(io) = pr.io.as_mut() {
                        io.pump(k, &mut pr.queued);
                    }
                }
            }
            if n < 64 {
                break;
            }
        }
    }
    // Before the host reports its windows (`frame_wins`) whether one will draw is unknown: the
    // output waits for that report.
    let held: Vec<(usize, Option<usize>)> = w
        .procs
        .iter()
        .filter_map(|(&p, pr)| {
            let seq = pr.after_frame?;
            let wins = w.frame_wins.as_ref();
            match crate::runtime::window_of(w, p) {
                Some(win) => wins
                    .is_none_or(|ws| {
                        ws.contains(&win) && w.win_frames.get(&win).copied().unwrap_or(0) <= seq
                    })
                    .then_some((p, Some(win))),
                // The Wallpapers thumbnailer: its handler resets ThumbnailImage sources from
                // the delegates' laid-out sizes, which the next frame settles.
                None => wins
                    .is_none_or(|ws| !ws.is_empty() && w.frame_seq <= seq)
                    .then_some((p, None)),
            }
        })
        .collect();
    w.frame_wanted = held
        .iter()
        .map(|&(_, win)| win.unwrap_or(usize::MAX))
        .collect();
    w.procs_ending.retain(|(_, pr)| !pr.job_ended());
    for (p, pr) in w.procs.iter_mut() {
        if let Some(rx) = &pr.job
            && !held.iter().any(|&(h, _)| h == *p)
        {
            pr.queued.extend(rx.try_iter());
        }
    }
    let ids: Vec<usize> = w
        .procs
        .iter()
        .filter(|(p, pr)| !pr.queued.is_empty() || w.dead.contains(p))
        .map(|(&p, _)| p)
        .collect();
    for p in ids {
        if w.dead.contains(&p) {
            if let Some(pr) = w.procs.remove(&p) {
                pr.kill_group();
            }
            continue;
        }
        if crate::imgjobs::is_type(w, p) {
            *changed = true;
            drain_image(w, p);
            continue;
        }
        while let Some(ev) = w.procs.get_mut(&p).and_then(|pr| pr.queued.pop_front()) {
            *changed = true;
            match ev {
                ProcEv::Out(s) => {
                    let mut b = std::mem::take(&mut w.procs.get_mut(&p).unwrap().out);
                    b.push_str(&s);
                    feed(w, p, "stdout", &mut b, false);
                    w.procs.get_mut(&p).unwrap().out = b;
                }
                ProcEv::Err(s) => {
                    let mut b = std::mem::take(&mut w.procs.get_mut(&p).unwrap().err);
                    b.push_str(&s);
                    feed(w, p, "stderr", &mut b, false);
                    w.procs.get_mut(&p).unwrap().err = b;
                }
                // Only an image item's run sends it (`drain_image`).
                ProcEv::File { .. } => {}
                ProcEv::Exit(_) | ProcEv::Signaled(_) => {
                    let (code, crash) = match ev {
                        ProcEv::Signaled(sig) => (sig, 1.0),
                        ProcEv::Exit(c) => (c, 0.0),
                        _ => unreachable!(),
                    };
                    let mut pr = w.procs.remove(&p).unwrap();
                    // The job ended by itself: its drop has nothing to stop.
                    pr.stop = None;
                    feed(w, p, "stdout", &mut pr.out, true);
                    feed(w, p, "stderr", &mut pr.err, true);
                    w.write(p, "running", Value::Bool(false));
                    w.write(p, "processId", Value::Null);
                    emit(
                        w,
                        p,
                        "exited",
                        &["exitCode", "exitStatus"],
                        vec![Value::Num(code as f64), Value::Num(crash)],
                    );
                    crate::weather::exited(w, p, code);
                    if pr.restart {
                        w.write(p, "running", Value::Bool(true));
                    }
                    break;
                }
            }
        }
    }
}

impl Runtime {
    /// Run native side effects (file reads, clock). Returns whether anything changed.
    /// Milliseconds until a timer or clock needs the main loop again (capped at one second).
    pub fn next_deadline(&mut self, now: f64) -> f64 {
        if self.later_pending() {
            return 0.0;
        }
        let mut w = self.world.borrow_mut();
        let (steppers, queued) = w.steppers_deadline(now);
        if queued {
            return 0.0;
        }
        let d: f64 = crate::desktop::rescan_in_ms(&w, now).unwrap_or(f64::INFINITY);
        d.min(steppers).max(0.0)
    }

    fn poll_xhr(&mut self) -> bool {
        use crate::xhr::Event;
        // What each request reported since the last poll; body parts that arrived together are
        // one read, as QNetworkReply::readAll gives them.
        let mut events: Vec<(Value, u64, Event)> = vec![];
        self.world.borrow_mut().xhr.retain(|p| {
            let mut reading = false;
            loop {
                let e = match p.rx.try_recv() {
                    Ok(e) => e,
                    Err(std::sync::mpsc::TryRecvError::Empty) => return true,
                    Err(_) => return false,
                };
                let last = matches!(e, Event::Finished | Event::Failed);
                match (e, events.last_mut()) {
                    (Event::Data(d), Some((_, _, Event::Data(read)))) if reading => {
                        read.extend_from_slice(&d)
                    }
                    (e, _) => {
                        reading = matches!(e, Event::Data(_));
                        events.push((p.obj.clone(), p.serial, e));
                    }
                }
                if last {
                    return false;
                }
            }
        });
        let any = !events.is_empty();
        for (o, serial, e) in events {
            crate::xhr::deliver(&mut self.it, &o, serial, e);
        }
        if any {
            self.settle();
        }
        any
    }

    /// The fd that turns readable when a `Process` has output, exited or can take queued
    /// stdin, or a watched file changed, for the host's event loop: the host then calls
    /// [`Self::process_fd_ready`] when it does, and the set is no longer polled on every
    /// service pass.
    pub fn process_fd(&mut self) -> Option<i32> {
        let mut w = self.world.borrow_mut();
        let fd = proc_ep(&mut w);
        w.proc_ep_watched = fd.is_some();
        fd
    }

    /// The host's loop saw [`Self::process_fd`] readable: run the Process service next pass.
    pub fn process_fd_ready(&self) {
        self.world.borrow().waker.mark(crate::svc::Svc::Process);
    }

    /// Whether [`Self::poll_native`] has work now, for a host without an event loop of its own
    /// (headless tools on a virtual clock): a service woke, a Process pipe or pidfd or a watched
    /// file is readable, Process events were read, or effects, objects or writes since the last
    /// poll are owed to services. Job output held for a frame is not: the host's next frame
    /// releases it (`World::win_frames`).
    pub fn native_ready(&mut self) -> bool {
        self.wait_fds(false, &[]).0
    }

    /// Blocks until [`Self::native_ready`], waiting in poll(2) on the Process and file-watch
    /// epoll set and on the services' wakeups, without a timeout. Installs the wakeup itself, so
    /// it is for hosts that never set one (`Waker::set_ping`).
    pub fn wait_native(&mut self) {
        self.wait_native_or(&[]);
    }

    /// [`Self::wait_native`] that also returns once one of the host's own fds in `extra` is
    /// readable (the pidfds of children it waits for, ...).
    pub fn wait_native_or(&mut self, extra: &[i32]) {
        while self.wait_fds(true, extra) == (false, false) {}
    }

    /// (native work ready, some `extra` fd readable), waiting for either while `block`.
    fn wait_fds(&mut self, block: bool, extra: &[i32]) -> (bool, bool) {
        use std::os::fd::{AsRawFd, FromRawFd};
        let (wake, ep) = {
            let mut w = self.world.borrow_mut();
            if w.wake_fd.is_none() {
                // SAFETY: eventfd(2) takes plain flags.
                let fd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
                assert!(fd >= 0, "eventfd: {}", std::io::Error::last_os_error());
                // SAFETY: `fd` is the eventfd just created, open and owned by nothing else.
                let fd = std::sync::Arc::new(unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) });
                let ping = fd.clone();
                w.waker.set_ping(move || {
                    let one = 1u64.to_ne_bytes();
                    // SAFETY: writes 8 bytes from a live buffer to the open eventfd.
                    unsafe { libc::write(ping.as_raw_fd(), one.as_ptr().cast(), one.len()) };
                });
                w.wake_fd = Some(fd);
            }
            let ep = proc_ep(&mut w);
            w.proc_ep_watched = ep.is_some();
            let shape = (w.items.len(), w.dead.len());
            if !w.waker.peek().is_empty()
                || !w.effects.is_empty()
                || w.svc_dirty != 0
                || w.native_shape != Some(shape)
                || w.procs.values().any(|pr| !pr.queued.is_empty())
            {
                return (true, false);
            }
            (
                w.wake_fd.as_ref().map_or(-1, |f| f.as_raw_fd()),
                ep.unwrap_or(-1),
            )
        };
        let mut fds: Vec<libc::pollfd> = [wake, ep]
            .iter()
            .chain(extra)
            .map(|&fd| libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            })
            .collect();
        let timeout = if block { -1 } else { 0 };
        // SAFETY: `fds` is a live array of `fds.len()` pollfds for the duration of the call.
        let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout) };
        if n <= 0 {
            return (false, false);
        }
        if fds[0].revents != 0 {
            let mut buf = [0u8; 8];
            // SAFETY: reads at most 8 bytes into a live buffer from the open eventfd.
            unsafe { libc::read(wake, buf.as_mut_ptr().cast(), buf.len()) };
        }
        if fds[1].revents != 0 {
            self.process_fd_ready();
        }
        (
            !self.world.borrow().waker.peek().is_empty(),
            fds[2..].iter().any(|p| p.revents != 0),
        )
    }

    /// Batches of file-watch events [`Self::poll_native`] has taken so far.
    pub fn file_events(&self) -> u64 {
        crate::fswatch::batches(&self.world.borrow())
    }

    fn poll_keyring(&mut self) -> bool {
        crate::keyring::poll(&mut self.world.borrow_mut())
    }

    pub fn poll_native(&mut self) -> bool {
        use crate::svc::Svc;
        let mut changed = false;
        // A service runs when its channel woke it (QSocketNotifier), it has calls queued, or an
        // object whose properties it mirrors was written (`World::mirror`); every service runs
        // when objects were created or destroyed, so it finds its singleton or the objects it
        // drives. Backend timeouts run on the service's own thread, which wakes it when they
        // fire; main-thread timeouts (desktop rescans) are deadlines in `next_deadline`. Writes
        // and objects made during this poll count for the next one.
        let (ready, all, owed) = {
            let mut w = self.world.borrow_mut();
            let shape = (w.items.len(), w.dead.len());
            let all = w.native_shape != Some(shape);
            w.native_shape = Some(shape);
            (w.waker.take(), all, std::mem::take(&mut w.svc_dirty))
        };
        let ran = std::cell::Cell::new(0u64);
        let due = |s: Svc, calls: bool| {
            let run = all || calls || ready.has(s) || owed & s.bit() != 0;
            if run {
                ran.set(ran.get() | s.bit());
            }
            run
        };
        let mut lap = Lap::new();
        let mut dispatches = vec![];
        let mut mpris_calls = vec![];
        let mut sni_calls = vec![];
        let mut notif_calls = vec![];
        let mut bluez_calls = vec![];
        let mut network_calls = vec![];
        {
            let mut w = self.world.borrow_mut();
            let effects = std::mem::take(&mut w.effects);
            for e in effects {
                use crate::svc::Effect;
                match e {
                    Effect::ExecDetached(a) => {
                        exec_detached(&mut w, a.first().unwrap_or(&Value::Undefined))
                    }
                    Effect::Reload(_) => RELOAD.store(true, std::sync::atomic::Ordering::Relaxed),
                    Effect::HyprDispatch(a) => {
                        dispatches.push(a.first().map(|v| v.to_js_string()).unwrap_or_default())
                    }
                    Effect::Session(c) => crate::session::run_effect(&c),
                    Effect::Mpris(c) => mpris_calls.push(c),
                    Effect::Sni(c) => sni_calls.push(c),
                    Effect::Notif(c) => notif_calls.push(c),
                    Effect::Bluez(c) => bluez_calls.push(c),
                    Effect::Network(c) => network_calls.push(c),
                    Effect::Xhr(a) => xhr_start(&mut w, a),
                    Effect::XhrAbort(a) => w.xhr.retain(|p| match (&p.obj, a.first()) {
                        (Value::Obj(o), Some(Value::Obj(x))) => !std::rc::Rc::ptr_eq(o, x),
                        _ => true,
                    }),
                    Effect::Method(a) => {
                        let ty = match a.first() {
                            Some(Value::Item(i)) => w.items.get(*i).map_or(String::new(), |it| {
                                crate::runtime::join_syms(&it.types, "<")
                            }),
                            _ => String::new(),
                        };
                        let m = a.get(1).map(|v| v.to_js_string()).unwrap_or_default();
                        unhandled_effect(&format!("method {ty}.{m}"))
                    }
                    Effect::Unhandled(k, _) => unhandled_effect(k),
                }
            }
        }
        if due(Svc::Hypr, !dispatches.is_empty()) {
            changed |= self.poll_hyprland(dispatches);
            lap.at("poll_hyprland");
        }
        if due(Svc::Hyprctl, false) {
            changed |= self.poll_hyprctl();
            lap.at("poll_hyprctl");
        }
        if due(Svc::Mpris, !mpris_calls.is_empty()) {
            changed |= self.poll_mpris(mpris_calls);
            lap.at("poll_mpris");
        }
        if due(Svc::Sni, !sni_calls.is_empty()) {
            changed |= self.poll_sni(sni_calls);
            lap.at("poll_sni");
        }
        if due(Svc::Syntax, false) {
            changed |= crate::syntax::poll(&mut self.world.borrow_mut());
            lap.at("poll_syntax");
        }
        if due(Svc::Theme, false) {
            changed |= crate::colorscheme::poll(&mut self.world.borrow_mut());
            lap.at("poll_theme");
        }
        if due(Svc::Impulse, false) {
            changed |= crate::impulse::deliver(&mut self.world.borrow_mut());
            lap.at("poll_impulse");
        }
        // Pipewire and BlueZ react to their own QML writes (`World::write_watch`), not to any write.
        let world = self.world.clone();
        let watched = move |s: Svc, ty: &str| {
            let w = world.borrow();
            ready.has(s)
                || w.has_watched(s)
                || w.singletons.contains_key(ty)
                    && match s {
                        Svc::Pipewire => w.pw.is_none(),
                        _ => w.bluez.is_none(),
                    }
        };
        if watched(Svc::Pipewire, "Pipewire") {
            changed |= self.poll_pipewire();
            lap.at("poll_pipewire");
        }
        if due(
            Svc::Notif,
            !notif_calls.is_empty() || self.world.borrow().notif_backlog,
        ) {
            changed |= self.poll_notifications(notif_calls);
            lap.at("poll_notifications");
        }
        if due(Svc::Upower, false) {
            changed |= self.poll_upower();
            lap.at("poll_upower");
        }
        if due(Svc::Brightness, false) {
            changed |= crate::brightness::poll(&mut self.world.borrow_mut());
            lap.at("poll_brightness");
        }
        if !bluez_calls.is_empty() || watched(Svc::Bluez, "Bluetooth") {
            changed |= self.poll_bluez(bluez_calls);
            lap.at("poll_bluez");
        }
        if due(Svc::Network, !network_calls.is_empty()) {
            changed |= self.poll_network(network_calls);
            lap.at("poll_network");
        }
        if due(Svc::Pam, false) {
            changed |= self.poll_pam();
            lap.at("poll_pam");
        }
        if due(Svc::Polkit, false) {
            changed |= self.poll_polkit();
            lap.at("poll_polkit");
        }

        crate::fswatch::take_events(&mut self.world.borrow_mut());
        changed |= self.poll_folders();
        lap.at("poll_folders");
        changed |= self.poll_day_of_week_rows();
        lap.at("poll_day_of_week_rows");

        let fvs = {
            let mut w = self.world.borrow_mut();
            let fvs = live_of(&mut w, "FileViewInternal");
            crate::fswatch::retain_files(&mut w, &fvs);
            fvs
        };
        for &i in &fvs {
            let mut w = self.world.borrow_mut();
            let path = file_path(&mut w, i);
            let watch = !path.is_empty() && w.read(i, "watchChanges").is_some_and(|v| v.truthy());
            if crate::fswatch::file_changed(&mut w, i, watch.then_some(path.as_str())) {
                if crate::fswatch::renewed_only(&w, &path) && same_text(&mut w, i, &path) {
                    // Watched again at a new inode holding what the view already shows.
                } else if !own_write_echo(&w, i, &path) {
                    w.fire.push(crate::runtime::Fire::Handler(
                        i,
                        crate::runtime::Sym::new("onFileChanged"),
                    ));
                    changed = true;
                } else if !w.read(i, "loaded").is_some_and(|v| v.truthy()) {
                    // The first write of a view that never loaded (Config.qml's `writeAdapter()`
                    // on a fresh HOME): Quickshell reports it as fileChanged and the original reloads; load
                    // it directly, so `onLoaded` (`Config.ready`) fires without that echo.
                    load_file(&mut w, i);
                    changed = true;
                }
            }
            // A view whose path was set reads it when it was told to, not whenever it differs.
            let job = w.file_views.get(&i).is_none_or(|v| v.job);
            if path.is_empty() || w.file_loaded.get(&i) == Some(&path) || !job {
                continue;
            }
            load_file(&mut w, i);
            changed = true;
        }

        if due(Svc::Process, procs_pending(&self.world.borrow())) {
            changed |= poll_processes(&mut self.world.borrow_mut());
            lap.at("poll_processes");
        }
        changed |= crate::ports::clip::poll(&mut self.world.borrow_mut());
        changed |= self.poll_layout();
        changed |= crate::ports::clip::poll_text(&mut self.world.borrow_mut());
        if due(Svc::Keyring, false) {
            changed |= self.poll_keyring();
        }
        if due(Svc::Fprint, false) {
            changed |= crate::fprint::poll(&mut self.world.borrow_mut());
            lap.at("poll_keyring");
        }
        if ready.has(Svc::Cava) || self.world.borrow().has_watched(Svc::Cava) {
            changed |= crate::cava::poll(&mut self.world.borrow_mut());
            lap.at("poll_cava");
        }
        if due(Svc::Position, false) {
            changed |= crate::position::poll(&mut self.world.borrow_mut());
        }
        if due(Svc::Weather, false) {
            let w = &mut *self.world.borrow_mut();
            if let Some(fetch) = crate::weather::due(w) {
                emit(w, fetch, "networkRestored", &[], vec![]);
                changed = true;
            }
        }
        changed |= crate::desktop::poll(&mut self.world.borrow_mut());
        lap.at("poll");
        if due(Svc::Xhr, false) {
            changed |= self.poll_xhr();
            lap.at("poll_xhr");
        }
        changed |= self.poll_config_edits();

        lap.at("fileviews");
        let clocks = live_of(&mut self.world.borrow_mut(), "SystemClock");
        for c in clocks {
            changed |= clock_tick(&self.world, &mut self.it, c);
        }
        if changed {
            self.settle();
        }
        self.world.borrow_mut().native_ran = crate::svc::Ready(ran.get());
        lap.at("clocks+rest");
        changed
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::runtime::Runtime;
    use crate::value::Value;

    /// With SIGCHLD ignored the kernel reaps a `Process` child itself and wait(2) fails with
    /// `ECHILD`: the exit is still reported (exit code -1, the status being unknown) and the
    /// failure is logged. In a process of its own: the disposition and stderr are the
    /// process's.
    #[test]
    fn a_failed_wait_is_logged_and_still_reports_the_exit() {
        use std::io::{Read as _, Seek as _};
        use std::os::fd::AsRawFd as _;
        const TEST: &str = concat!(
            module_path!(),
            "::a_failed_wait_is_logged_and_still_reports_the_exit"
        );
        if !crate::testenv::in_own_process(TEST) {
            crate::testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
            return;
        }
        // SAFETY: signal(2) with a valid signal number and SIG_IGN.
        unsafe { libc::signal(libc::SIGCHLD, libc::SIG_IGN) };
        // The child exits at the end of its input, closed once its pidfd is open: one that
        // exited on its own could be reaped before that.
        let mut ch = std::process::Command::new("cat")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let pid = ch.id();
        let pidfd = workpool::pidfd_open(pid as i32).unwrap();
        drop(ch.stdin.take());
        let mut gone = libc::pollfd {
            fd: pidfd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: poll(2) over one live pollfd; the pidfd turns readable once the child exited.
        assert_eq!(unsafe { libc::poll(&mut gone, 1, -1) }, 1);
        let mut io = super::ProcIo {
            ch: Some(ch),
            pipes: [None, None],
            pend: Default::default(),
            pidfd: Some(pidfd),
        };
        let mut log = tempfile::tempfile().unwrap();
        // SAFETY: dup(2) of this process's open stderr.
        let stderr = unsafe { libc::dup(2) };
        assert!(stderr >= 0);
        // SAFETY: dup2(2) between two open descriptors; `log` stays open for the call.
        assert_eq!(unsafe { libc::dup2(log.as_raw_fd(), 2) }, 2);
        let mut out = std::collections::VecDeque::new();
        io.pump(2, &mut out);
        // SAFETY: dup2(2) between two open descriptors, putting stderr back.
        assert_eq!(unsafe { libc::dup2(stderr, 2) }, 2);
        // SAFETY: close(2) of the copy made above, used by nothing else.
        unsafe { libc::close(stderr) };
        assert!(
            matches!(out.make_contiguous(), [super::ProcEv::Exit(-1)]),
            "the exit was not reported once, as exit code -1"
        );
        let mut text = String::new();
        log.rewind().unwrap();
        log.read_to_string(&mut text).unwrap();
        assert!(
            text.contains(&format!("impulse-shell: cannot wait for child {pid}: ")),
            "stderr: {text:?}"
        );
    }

    /// A request goes to the windows that redraw for its change in the same turn, for their next
    /// frame only: one that redraws nothing new, or was left alone, drops it, so a later change
    /// never fades on it. With nothing armed a frame is never compared with the last one.
    #[test]
    fn crossfade_requests_fade_only_the_change_they_come_with() {
        use super::{FadeArm, bump_crossfade};
        let ms = std::time::Duration::from_millis;
        let (mut drawn, mut idle) = (FadeArm::new(), FadeArm::new());
        let unarmed = || -> Option<bool> { panic!("compared frames with no fade armed") };
        assert_eq!(drawn.take(unarmed), None);
        bump_crossfade(300);
        drawn.observe(true);
        idle.observe(false);
        assert_eq!(drawn.take(|| Some(true)), Some(ms(300)));
        assert_eq!(drawn.take(unarmed), None, "one fade per request");
        assert_eq!(idle.take(unarmed), None, "a window the change left alone");

        bump_crossfade(450);
        drawn.observe(true);
        assert_eq!(drawn.take(|| Some(false)), None, "an unchanged frame");
        assert_eq!(drawn.take(unarmed), None, "the next change is not faded");
        bump_crossfade(450);
        drawn.observe(true);
        assert_eq!(drawn.take(|| None), None, "no previous frame to fade from");
        assert_eq!(drawn.take(unarmed), None);

        bump_crossfade(600);
        bump_crossfade(200);
        idle.observe(true);
        assert_eq!(
            idle.take(|| Some(true)),
            Some(ms(200)),
            "the last request of the turn"
        );
    }

    /// A service polls for writes to the objects it mirrors, not for every world write
    /// (QSocketNotifier per service instead of a sweep on any revision bump).
    #[test]
    fn services_poll_for_their_own_objects_only() {
        use crate::svc::Svc;
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(
                "import QtQuick\nimport Rustshell.Services.Pam\nItem { property int other: 0\n PamContext { id: p; configDirectory: '/etc/pam.d' } }",
            )
            .unwrap();
        let p = rt.id("p").unwrap();
        rt.poll_native();
        assert!(
            rt.world.borrow().native_ran.has(Svc::Pam),
            "new objects: every service runs"
        );
        rt.poll_native();
        rt.set(root, "other", Value::Num(1.0));
        rt.poll_native();
        let ran = rt.world.borrow().native_ran;
        assert!(!ran.has(Svc::Pam) && !ran.has(Svc::Upower), "{ran:?}");
        rt.set(p, "user", Value::str("someone"));
        rt.poll_native();
        let ran = rt.world.borrow().native_ran;
        assert!(ran.has(Svc::Pam), "a PamContext write reaches its service");
        assert!(!ran.has(Svc::Pipewire) && !ran.has(Svc::Syntax), "{ran:?}");
    }

    /// A FileView on a FIFO or a device fails at once (code 4, not a directory/regular file)
    /// instead of blocking the UI thread or reading `/dev/zero` until OOM.
    #[test]
    fn file_view_reads_only_regular_files() {
        let t = tempfile::tempdir().unwrap();
        let fifo = t.path().join("fifo");
        let c = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
        // SAFETY: `c` is a live NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        for p in [fifo.to_str().unwrap(), "/dev/zero"] {
            assert_eq!(super::read_input(p), None, "{p}");
        }
        let f = t.path().join("f");
        std::fs::write(&f, "text").unwrap();
        assert_eq!(
            super::read_input(f.to_str().unwrap()).as_deref(),
            Some("text")
        );
    }

    #[test]
    fn system_clock_has_wall_time_before_first_poll() {
        // The original Hyprsunset.qml decides auto night light from DateTime.clock in onAutomaticChanged;
        // Quickshell's SystemClock never exposes a 00:00 placeholder to startup handlers.
        crate::builtins::set_clock(Some(testenv::local_time_ms(19, 42)));
        let mut rt = Runtime::new(None);
        let r = rt
            .load_str(
                "import QtQuick\nimport Quickshell\nItem { id: root\n property real seen: -1\n\
                 SystemClock { id: clock; precision: SystemClock.Minutes }\n\
                 Component.onCompleted: root.seen = clock.hours * 60 + clock.minutes }",
            )
            .unwrap();
        crate::builtins::set_clock(None);
        assert_eq!(rt.get(r, "seen").to_number(), (19 * 60 + 42) as f64);
    }

    #[test]
    fn loaded_json_replaces_initializers_and_bindings_follow_inputs() {
        // The original Config.qml: values loaded from config.json replace the adapter's initializers.
        let dir = std::env::temp_dir().join(format!("ri-json-keep-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("c.json");
        std::fs::write(
            &path,
            "{\"b\": true, \"s\": \"w.png\", \"k\": 7, \"n\": {\"c\": 2}}",
        )
        .unwrap();
        let mut rt = Runtime::new(None);
        let r = rt
            .load_str(&format!(
                "import QtQuick\nimport Rustshell.Io\nItem {{ id: root; property int base: 3\n\
                 property alias o: ad\n\
                 FileViewInternal {{ id: fv; property string path: \"{}\"\n\
                 JsonAdapter {{ id: ad; property bool b: false; property string s: \"\"\n\
                 property int k: root.base\n\
                 property JsonObject n: JsonObject {{ property int c: 0 }} }} }} }}",
                path.display()
            ))
            .unwrap();
        rt.settle();
        rt.poll_native();
        rt.settle();
        let ad = match rt.get(r, "o") {
            crate::value::Value::Item(a) => a,
            v => panic!("{v:?}"),
        };
        let n = match rt.get(ad, "n") {
            crate::value::Value::Item(n) => n,
            v => panic!("{v:?}"),
        };
        let got = (
            rt.get(ad, "b").truthy(),
            rt.get(ad, "s").to_js_string(),
            rt.get(ad, "k").to_number(),
            rt.get(n, "c").to_number(),
        );
        // A binding with inputs still follows them, as in Qt.
        rt.world
            .borrow_mut()
            .write(r, "base", crate::value::Value::Num(5.0));
        rt.settle();
        let k = rt.get(ad, "k").to_number();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(got, (true, "w.png".to_string(), 7.0, 2.0));
        assert_eq!(k, 5.0);
    }

    #[test]
    fn load_does_not_update_adapter() {
        // Qt JsonAdapter only emits adapterUpdated on a real change: starting the shell must
        // leave config.json untouched (the original Config.qml writes the file on adapterUpdated).
        let dir = std::env::temp_dir().join(format!("ri-json-quiet-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("c.json");
        std::fs::write(
            &path,
            "{\"l\": [\"a\"], \"o\": {\"x\": 1}, \"n\": {\"m\": [1]}}",
        )
        .unwrap();
        let mut rt = Runtime::new(None);
        let r = rt
            .load_str(&format!(
                "import QtQuick\nimport Rustshell.Io\nItem {{ id: root; property int upd: 0\n\
                 property alias o: ad\n\
                 FileViewInternal {{ id: fv; property string path: \"{}\"\n\
                 onAdapterUpdated: root.upd += 1\n\
                 JsonAdapter {{ id: ad; property list<string> l: [\"a\"]; property var o: ({{ x: 1 }})\n\
                 property list<string> missing: [\"d\"]\n\
                 property JsonObject n: JsonObject {{ property list<var> m: [1]; property var q: [] }} }} }} }}",
                path.display()
            ))
            .unwrap();
        rt.settle();
        rt.poll_native();
        rt.settle();
        let quiet = rt.get(r, "upd").to_number();
        let ad = match rt.get(r, "o") {
            crate::value::Value::Item(a) => a,
            v => panic!("{v:?}"),
        };
        rt.set(ad, "missing", crate::value::Value::str("e"));
        let changed = rt.get(r, "upd").to_number();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!((quiet, changed), (0.0, 1.0));
    }

    #[test]
    fn write_adapter_breaks_empty_containers_like_qjsondocument() {
        let dir = std::env::temp_dir().join(format!("ri-json-empty-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("c.json");
        let _ = std::fs::remove_file(&path);
        let mut rt = Runtime::new(None);
        let r = rt
            .load_str(&format!(
                "import QtQuick\nimport Rustshell.Io\nItem {{ property int step: 0\n\
                 onStepChanged: fv.writeAdapter()\n\
                 FileViewInternal {{ id: fv; property string path: \"{}\"\n\
                 JsonAdapter {{ property list<string> l: []; property var o: ({{}})\n\
                 property JsonObject n: JsonObject {{ property list<var> m: [] }} }} }} }}",
                path.display()
            ))
            .unwrap();
        rt.settle();
        rt.world
            .borrow_mut()
            .write(r, "step", crate::value::Value::Num(1.0));
        rt.settle();
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            text,
            "{\n    \"l\": [\n    ],\n    \"n\": {\n        \"m\": [\n        ]\n    },\n    \"o\": {\n    }\n}\n"
        );
    }

    #[test]
    fn write_adapter_replaces_the_file_by_rename() {
        // Quickshell `atomicWrites` is on unless a FileView turns it off: the settings window
        // saves config.json while its own colour generation reads it on another thread, and a
        // file truncated for the write reads as one with no `terminalGenerationProps`.
        let dir = std::env::temp_dir().join(format!("ri-json-atomic-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let (path, held) = (dir.join("c.json"), dir.join("held"));
        let _ = std::fs::remove_file(&held);
        std::fs::write(&path, "{\n    \"k\": 1\n}\n").unwrap();
        // A second name for the file as it is now: what a reader that has it open still sees.
        std::fs::hard_link(&path, &held).unwrap();
        let mut rt = Runtime::new(None);
        let r = rt
            .load_str(&format!(
                "import QtQuick\nimport Rustshell.Io\nItem {{ property int step: 0\n\
                 onStepChanged: {{ ad.k = step; fv.writeAdapter() }}\n\
                 FileViewInternal {{ id: fv; property string path: \"{}\"\n\
                 JsonAdapter {{ id: ad; property int k: 0 }} }} }}",
                path.display()
            ))
            .unwrap();
        rt.settle();
        rt.poll_native();
        rt.settle();
        rt.world
            .borrow_mut()
            .write(r, "step", crate::value::Value::Num(2.0));
        rt.settle();
        let (new, old) = (
            std::fs::read_to_string(&path).unwrap(),
            std::fs::read_to_string(&held).unwrap(),
        );
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(new, "{\n    \"k\": 2\n}\n");
        assert_eq!(old, "{\n    \"k\": 1\n}\n");
    }

    #[test]
    fn json_load_keeps_file_values() {
        // A config.json loaded after the adapter keeps its values over the JsonAdapter's defaults.
        let dir = std::env::temp_dir().join(format!("ri-json-reeval-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("c.json");
        std::fs::write(&path, "{\"s\": \"file\", \"d\": 5, \"n\": {\"b\": true}}").unwrap();
        let mut rt = Runtime::new(None);
        rt.load_str(&format!(
            "import QtQuick\nimport Rustshell.Io\nItem {{ id: root; property int base: 1\n\
             FileViewInternal {{ id: fv; property string path: \"{}\"\n\
             JsonAdapter {{ id: ad; property string s: \"x\"; property int d: root.base * 2\n\
             property JsonObject n: JsonObject {{ property bool b: false }} }} }} }}",
            path.display()
        ))
        .unwrap();
        rt.settle();
        let (root, fv, ad) = {
            let w = rt.world.borrow();
            let find = |t: &str| {
                (0..w.items.len())
                    .find(|&i| w.items[i].types.iter().any(|x| x == t))
                    .unwrap()
            };
            (find("Item"), find("FileViewInternal"), find("JsonAdapter"))
        };
        super::load_file(&mut rt.world.borrow_mut(), fv);
        rt.settle();
        let n = match rt.get(ad, "n") {
            crate::value::Value::Item(n) => n,
            v => panic!("{v:?}"),
        };
        assert_eq!(rt.get(ad, "s").to_js_string(), "file");
        assert!(rt.get(n, "b").truthy());
        // QMetaProperty::write keeps a real binding: it still re-runs when its inputs change.
        assert_eq!(rt.get(ad, "d").to_js_string(), "5");
        rt.world
            .borrow_mut()
            .write(root, "base", crate::value::Value::Num(3.0));
        rt.settle();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(rt.get(ad, "d").to_js_string(), "6");
    }

    #[test]
    fn late_written_control_props_notify_bindings() {
        let mut rt = Runtime::new(None);
        let r = rt
            .load_str(
                "import QtQuick\nimport QtQuick.Controls\nButton { id: b; property string c: b.hovered ? 'h' : 'n'\n\
                 Item { id: k; property bool sh: parent.hovered === undefined || parent.hovered } }",
            )
            .unwrap();
        rt.settle();
        assert_eq!(rt.get(r, "c").to_js_string(), "n");
        rt.world
            .borrow_mut()
            .write(r, "hovered", crate::value::Value::Bool(true));
        rt.settle();
        assert_eq!(rt.get(r, "c").to_js_string(), "h");
    }

    #[test]
    fn pushed_created_objects_notify_find_bindings() {
        // The original Network.qml: `rNetworks.push(apComp.createObject(root, ...))` on a readonly list,
        // `active: wifiNetworks.find(n => n.active)`, icon from `active?.strength`.
        let mut rt = Runtime::new(None);
        let r = rt
            .load_str(
                "import QtQuick\nItem { id: root\n readonly property list<QtObject> nets: []\n\
                 readonly property QtObject active: nets.find(n => n.active) ?? null\n\
                 property string sym: (root.active?.strength ?? 0) > 67 ? 'full' : 'empty'\n\
                 Component { id: apComp; QtObject { required property var lastIpcObject\n\
                   readonly property int strength: lastIpcObject.strength\n\
                   readonly property bool active: lastIpcObject.active } }\n\
                 function add(o) { const rn = root.nets; rn.push(apComp.createObject(root, { lastIpcObject: o })) }\n\
                 function weaken() { root.nets[0].lastIpcObject = { active: true, strength: 10 } }\n\
                 property int step: 0\n\
                 onStepChanged: { if (step === 1) add({ active: true, strength: 71 }); else weaken() } }",
            )
            .unwrap();
        rt.settle();
        assert_eq!(rt.get(r, "sym").to_js_string(), "empty");
        rt.world
            .borrow_mut()
            .write(r, "step", crate::value::Value::Num(1.0));
        rt.settle();
        assert_eq!(rt.get(r, "sym").to_js_string(), "full");
        rt.world
            .borrow_mut()
            .write(r, "step", crate::value::Value::Num(2.0));
        rt.settle();
        assert_eq!(rt.get(r, "sym").to_js_string(), "empty");
    }

    #[test]
    fn workspace_activate_dispatches_a_focus_by_name() {
        // Quickshell's HyprlandWorkspace::activate (src/wayland/hyprland/ipc/workspace.cpp),
        // its Lua branch: the name with `"` and `\` escaped.
        let mut rt = Runtime::new(None);
        let r = rt
            .load_str(
                "import QtQuick\nItem { property var ws: null; property int step: 0\n\
                 onStepChanged: ws.activate() }",
            )
            .unwrap();
        rt.settle();
        {
            let mut w = rt.world.borrow_mut();
            let ws = crate::types::native_singleton(&mut w, "HyprlandWorkspace");
            w.write(ws, "name", Value::str("a \"b\" \\c"));
            w.write(r, "ws", Value::Item(ws));
            w.write(r, "step", Value::Num(1.0));
        }
        rt.settle();
        assert_eq!(rt.errors(), Vec::<String>::new());
        let sent: Vec<String> = std::mem::take(&mut rt.world.borrow_mut().effects)
            .into_iter()
            .filter_map(|e| match e {
                crate::svc::Effect::HyprDispatch(a) => Some(a[0].to_js_string()),
                _ => None,
            })
            .collect();
        assert_eq!(sent, [r#"hl.dsp.focus({ workspace = "a \"b\" \\c" })"#]);
    }

    #[test]
    fn toplevel_activate_and_close_dispatch_by_address() {
        // The original DockAppButton.qml `toplevels[lastFocused].activate()`, DockApps.qml preview
        // `modelData.close()`: Quickshell's foreign-toplevel requests focus/close the window.
        let mut rt = Runtime::new(None);
        let r = rt
            .load_str(
                "import QtQuick\nItem { property var t: null; property int step: 0\n\
                 onStepChanged: { if (step === 2) t.close(); else t.activate() } }",
            )
            .unwrap();
        rt.settle();
        {
            let mut w = rt.world.borrow_mut();
            let t = crate::types::native_singleton(&mut w, "Toplevel");
            let ht = crate::types::native_singleton(&mut w, "HyprlandToplevel");
            w.write(ht, "address", Value::str("55877820bc30"));
            w.write(t, "HyprlandToplevel", Value::Item(ht));
            w.write(r, "t", Value::Item(t));
        }
        let dispatched = |rt: &mut Runtime, step: f64| {
            rt.world.borrow_mut().write(r, "step", Value::Num(step));
            rt.settle();
            std::mem::take(&mut rt.world.borrow_mut().effects)
                .into_iter()
                .filter_map(|e| match e {
                    crate::svc::Effect::HyprDispatch(a) => Some(a[0].to_js_string()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            dispatched(&mut rt, 1.0),
            ["hl.dsp.focus({window=\"address:0x55877820bc30\"})"]
        );
        let t = match rt.get(r, "t") {
            Value::Item(t) => t,
            v => panic!("{v:?}"),
        };
        rt.world
            .borrow_mut()
            .write(t, "activated", Value::Bool(true));
        assert!(
            dispatched(&mut rt, 3.0).is_empty(),
            "refocused the active window"
        );
        assert_eq!(
            dispatched(&mut rt, 2.0),
            ["hl.dsp.window.close({window=\"address:0x55877820bc30\"})"]
        );
    }

    #[test]
    fn user_signals_dispatch_with_args() {
        let mut rt = Runtime::new(None);
        let r = rt
            .load_str(
                "import QtQuick\nItem { id: r; property var a: 0; property var b: 0; property var c: 0\n\
                 signal hit(var x, string y)\n onHit: (x, y) => { r.a = x; r.b = y }\n\
                 Item { id: k; signal ping(int n); onPing: r.c = n * 2 }\n\
                 Component.onCompleted: { hit(3, 'q'); k.ping(7) } }",
            )
            .unwrap();
        rt.settle();
        assert_eq!(rt.get(r, "a").to_number(), 3.0);
        assert_eq!(rt.get(r, "b").to_js_string(), "q");
        assert_eq!(rt.get(r, "c").to_number(), 14.0);
    }

    #[test]
    fn process_collects_splits_and_exits() {
        let mut rt = Runtime::new(None);
        let r = rt
            .load_str(
                "import QtQuick\nimport Rustshell.Io\nItem { id: r; property string t; property var lines: []; property int code: -9\n\
                 Process { running: true; command: ['sh', '-c', 'echo a; printf b; exit 3']\n\
                   stdout: StdioCollector { onStreamFinished: r.t = text }\n\
                   onExited: (exitCode, exitStatus) => r.code = exitCode }\n\
                 Process { id: p2; command: ['printf', 'x|y|z']; stdout: SplitParser { splitMarker: '|'; onRead: data => r.lines = [...r.lines, data] } }\n\
                 Component.onCompleted: p2.running = true }",
            )
            .unwrap();
        rt.settle();
        run_until(&mut rt, &|rt| {
            rt.get(r, "code").to_number() == 3.0 && rt.get(r, "lines").to_js_string() == "x,y,z"
        });
        assert_eq!(rt.get(r, "t").to_js_string(), "a\nb");
        assert_eq!(rt.get(r, "code").to_number(), 3.0);
        assert_eq!(rt.get(r, "lines").to_js_string(), "x,y,z");
    }

    /// The MIME type DirectoryIcon asks for is the file's own (a symbolic link is
    /// `inode/symlink`); AttachedFileIndicator's default reads the file a link points to.
    #[test]
    fn file_mime_type_follows_symlinks_unless_told_not_to() {
        let d = tempfile::tempdir().unwrap();
        let png = d.path().join("a.png");
        std::fs::write(
            &png,
            b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x02\0\0\0",
        )
        .unwrap();
        let link = d.path().join("link.png");
        std::os::unix::fs::symlink(&png, &link).unwrap();
        let qml = d.path().join("Icons.qml");
        let proc = |id: &str, path: &std::path::Path, more: &str| {
            format!(
                "property string {id}\nproperty var {id}Proc: FileMimeType {{ running: true; \
                 path: {:?}; {more}\n\
                 stdout: StdioCollector {{ onStreamFinished: r.{id} = text }} }}\n",
                path.to_str().unwrap()
            )
        };
        std::fs::write(
            &qml,
            format!(
                "import QtQuick\nimport Rustshell.Io\nimport Impulse\nItem {{ id: r\n{}{}{} }}",
                proc("file", &png, "followSymlinks: false"),
                proc("own", &link, "followSymlinks: false"),
                proc("target", &link, ""),
            ),
        )
        .unwrap();
        let mut rt = Runtime::new(None);
        let r = rt.load_file(&qml).unwrap();
        rt.settle();
        let Value::Item(p) = rt.get(r, "targetProc") else {
            panic!()
        };
        assert_eq!(
            crate::procs::type_of(&rt.world.borrow(), p),
            Some("FileMimeType")
        );
        assert!(matches!(rt.get(p, "followSymlinks"), Value::Bool(true)));
        let text = |rt: &Runtime, n: &str| rt.get(r, n).to_js_string();
        run_until(&mut rt, &|rt| {
            ["file", "own", "target"]
                .iter()
                .all(|n| !text(rt, n).is_empty())
        });
        assert_eq!(text(&rt, "file"), "image/png");
        assert_eq!(text(&rt, "own"), "inode/symlink");
        assert_eq!(text(&rt, "target"), "image/png");
    }

    #[test]
    fn process_write_reaches_stdin() {
        // AcpSession: `proc.running = true` then `proc.write(...)` in the same call; later writes
        // go straight to the child's stdin.
        let mut rt = Runtime::new(None);
        let r = rt
            .load_str(
                "import QtQuick\nimport Rustshell.Io\nItem { id: r; property var lines: []\n\
                 function more() { p.write('c\\n'); p.stdinEnabled = false }\n\
                 Process { id: p; command: ['cat']; stdinEnabled: true\n\
                   stdout: SplitParser { onRead: data => r.lines = [...r.lines, data] } }\n\
                 Process { id: q; command: ['cat']\n\
                   stdout: SplitParser { onRead: data => r.lines = [...r.lines, 'q' + data] } }\n\
                 Component.onCompleted: { p.running = true; p.write('a\\nb'); p.write('\\n');\n\
                   q.running = true; q.write('x\\n') } }",
            )
            .unwrap();
        rt.settle();
        run_until(&mut rt, &|rt| rt.get(r, "lines").to_js_string() == "a,b");
        let p = rt.id("p").unwrap();
        rt.call_method(r, "more", vec![]);
        rt.settle();
        run_until(&mut rt, &|rt| rt.get(r, "lines").to_js_string() == "a,b,c");
        // stdinEnabled = false closed the pipe: cat exits. q never had stdin: no output.
        run_until(&mut rt, &|rt| !rt.get(p, "running").truthy());
        assert_eq!(rt.get(r, "lines").to_js_string(), "a,b,c");
    }

    #[test]
    fn process_write_to_a_child_not_reading_neither_blocks_nor_loses_data() {
        // 1 MiB is far more than a pipe holds: the rest waits in the Process for EPOLLOUT. The
        // child reads nothing before the test writes `go`, after `feed` returned: a write that
        // blocked would never return.
        let dir = tempfile::tempdir().unwrap();
        let go = dir.path().join("go");
        crate::testenv::fifo(&go);
        let mut rt = Runtime::new(None);
        let r = rt
            .load_str(&format!(
                "import QtQuick\nimport Rustshell.Io\nItem {{ id: r; property string n\n\
                 function feed() {{ p.write('x'.repeat(1048576)); p.stdinEnabled = false }}\n\
                 Process {{ id: p; running: true; command: ['sh', '-c', 'read _ < \"$0\"; wc -c', '{}']; stdinEnabled: true\n\
                   stdout: SplitParser {{ onRead: data => r.n = data.trim() }} }} }}",
                go.display()
            ))
            .unwrap();
        rt.settle();
        let p = rt.id("p").unwrap();
        run_until(&mut rt, &|rt| rt.get(p, "running").truthy());
        rt.call_method(r, "feed", vec![]);
        assert!(
            rt.world.borrow().procs[&p]
                .stdin
                .as_ref()
                .is_some_and(|s| s.armed && s.closing && !s.buf.is_empty()),
            "the rest waits for the pipe, which closes once it is written"
        );
        std::fs::write(&go, "\n").unwrap();
        run_until(&mut rt, &|rt| rt.get(r, "n").to_js_string() == "1048576");
        run_until(&mut rt, &|rt| !rt.get(p, "running").truthy());
    }

    #[test]
    fn a_host_watching_the_process_fd_is_not_polled_again() {
        let mut rt = Runtime::new(None);
        let r = rt
            .load_str(
                "import QtQuick\nimport Rustshell.Io\nItem { id: r; property string out\n\
                 Process { running: true; command: ['echo', 'hi']\n\
                   stdout: SplitParser { onRead: data => r.out = data } } }",
            )
            .unwrap();
        rt.settle();
        let ep = {
            use std::os::fd::AsRawFd;
            rt.world.borrow().proc_ep.as_ref().unwrap().as_raw_fd()
        };
        let mut pfd = libc::pollfd {
            fd: ep,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: `pfd` is one live pollfd for the duration of the call.
        assert_eq!(unsafe { libc::poll(&mut pfd, 1, 5000) }, 1);
        assert!(
            super::procs_pending(&rt.world.borrow()),
            "a polling host polls the set"
        );
        assert_eq!(rt.process_fd(), Some(ep));
        assert!(
            !super::procs_pending(&rt.world.borrow()),
            "a watching host reports readiness itself"
        );
        rt.process_fd_ready();
        assert!(rt.world.borrow().waker.peek().has(crate::svc::Svc::Process));
        run_until(&mut rt, &|rt| rt.get(r, "out").to_js_string() == "hi");
    }

    thread_local! {
        /// This thread's end of its test runtimes' wakeups.
        static WAKE: crate::testenv::Wakeups = crate::testenv::Wakeups::new();
    }

    /// Makes `rt`'s service threads wake this thread's [`wait_event`]. Call it before the poll
    /// whose follow-ups are waited for: what arrived before it, that poll sees.
    pub(crate) fn watch(rt: &Runtime) {
        WAKE.with(|w| rt.world.borrow().waker.set_ping(w.ping()));
    }

    /// Sleeps until a service thread of a [`watch`]ed `rt` wakes its loop, or a `Process` has
    /// output, exited or can take stdin (the fd the shell's loop sleeps on). No deadline of its
    /// own: a hang is nextest's slow-timeout to end.
    pub(crate) fn wait_event(rt: &mut Runtime) {
        let fd = rt.process_fd();
        if WAKE.with(|w| w.wait(fd, None)).fd {
            rt.process_fd_ready();
        }
    }

    /// Consumes the wakeups delivered so far: the next [`wait_event`] waits for a new one.
    pub(crate) fn drain_events() {
        WAKE.with(|w| w.wait(None, Some(std::time::Duration::ZERO)));
    }

    /// Runs host loop turns until `f` holds, as the shell's loop does: services poll and
    /// bindings settle, again at once while a turn changed the world (its writes count for the
    /// next poll), else after [`wait_event`].
    pub(crate) fn run_until(rt: &mut Runtime, f: &dyn Fn(&Runtime) -> bool) {
        watch(rt);
        loop {
            let rev = rt.revision();
            rt.poll_native();
            rt.settle();
            if f(rt) {
                return;
            }
            if rt.revision() == rev {
                wait_event(rt);
            }
        }
    }

    fn threads() -> usize {
        std::fs::read_dir("/proc/self/task").unwrap().count()
    }

    #[test]
    fn process_starts_when_running_is_written() {
        // Quickshell Process::setRunning(true) starts the QProcess right there: the pid exists
        // before the handler returns, with no main-loop poll in between. (`true` would run as an
        // in-process applet, which has no pid.)
        let mut rt = Runtime::new(None);
        let r = rt
            .load_str(
                "import QtQuick\nimport Rustshell.Io\nItem { id: r; property bool go; property var pid: -1; property int exits\n\
                 onGoChanged: { p.running = true; r.pid = p.processId }\n\
                 Process { id: p; command: ['sleep', '0']; onExited: r.exits++ } }",
            )
            .unwrap();
        rt.settle();
        rt.set(r, "go", Value::Bool(true));
        assert!(rt.get(r, "pid").to_number() > 0.0);
        run_until(&mut rt, &|rt| rt.get(r, "exits").to_number() == 1.0);
    }

    #[test]
    fn processes_need_no_thread_each() {
        // threads() counts the whole process: parallel tests' threads would count too.
        let out = run_helper("thread_count_helper");
        assert!(
            out.status.success() && String::from_utf8_lossy(&out.stdout).contains("1 passed"),
            "{out:?}"
        );
    }

    /// Run by `processes_need_no_thread_each`.
    #[test]
    #[ignore = "child process: processes_need_no_thread_each runs it"]
    fn thread_count_helper() {
        if !is_helper("thread_count_helper") {
            return;
        }
        let mut rt = Runtime::new(None);
        // Each runs until `go` writes its stdin, so all 16 are running when threads are counted.
        let src = (0..16)
            .map(|i| format!("Process {{ id: p{i}; running: true; command: ['sh', '-c', 'read x; echo x']; stdinEnabled: true; stdout: StdioCollector {{ onStreamFinished: r.n++ }} }}"))
            .collect::<Vec<_>>()
            .join("\n");
        let go = (0..16)
            .map(|i| format!("p{i}.write('\\n');"))
            .collect::<String>();
        let r = rt
            .load_str(&format!(
                "import QtQuick\nimport Rustshell.Io\nItem {{ id: r; property int n\nfunction go() {{ {go} }}\n{src} }}"
            ))
            .unwrap();
        rt.settle();
        rt.poll_native();
        let t0 = threads();
        assert!(t0 < 16, "{t0} threads for 16 processes");
        rt.call_method(r, "go", vec![]);
        run_until(&mut rt, &|rt| rt.get(r, "n").to_number() == 16.0);
    }

    #[test]
    fn process_output_larger_than_a_read_and_exit_after_pipes_close() {
        let mut rt = Runtime::new(None);
        let r = rt
            .load_str(
                "import QtQuick\nimport Rustshell.Io\nItem { id: r; property int len; property string e; property int code: -9; property int code2: -9\n\
                 Process { running: true; command: ['sh', '-c', 'head -c 300000 /dev/zero | tr \\\\0 a; printf err >&2']\n\
                   stdout: StdioCollector { onStreamFinished: r.len = text.length }\n\
                   stderr: StdioCollector { onStreamFinished: r.e = text } }\n\
                 Process { id: q; running: true; command: ['sh', '-c', 'exec >&- 2>&-; read _; exit 4']; stdinEnabled: true\n\
                   onExited: c => r.code2 = c }\n\
                 function go() { q.write('\\n') } }",
            )
            .unwrap();
        rt.settle();
        // `q` closed its pipes and lives on until the test writes its stdin: its exit is
        // reported after the end of its pipes, not with it.
        let q = rt.id("q").unwrap();
        run_until(&mut rt, &|rt| {
            rt.get(r, "len").to_number() == 300000.0
                && rt.world.borrow().procs.get(&q).is_some_and(|p| {
                    p.io.as_ref()
                        .is_some_and(|io| io.pipes.iter().all(Option::is_none))
                })
        });
        assert_eq!(rt.get(r, "code2").to_number(), -9.0);
        rt.call_method(r, "go", vec![]);
        run_until(&mut rt, &|rt| rt.get(r, "code2").to_number() == 4.0);
        assert_eq!(rt.get(r, "e").to_js_string(), "err");
    }

    fn alive(pid: i32) -> bool {
        // A zombie still answers kill(0): count it as gone.
        std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| {
            s.rsplit(')')
                .next()
                .is_some_and(|r| !r.trim_start().starts_with('Z'))
        })
    }

    /// A Process whose shell prints its own pid, pgid and argv[0], then backgrounds a sleeper and
    /// prints that pid too.
    fn tree(rt: &mut Runtime) -> (usize, Vec<String>) {
        let r = rt
            .load_str(
                "import QtQuick\nimport Rustshell.Io\nItem { id: r; property string t\n\
                 Process { running: true; command: ['sh', '-c', 'echo $$ $(ps -o pgid= -p $$) $(tr \"\\\\0\" \" \" < /proc/$$/cmdline | cut -d\" \" -f1); sleep infinity & echo $!; wait']\n\
                   stdout: SplitParser { onRead: data => r.t = r.t + data + ' ' } } }",
            )
            .unwrap();
        rt.settle();
        run_until(rt, &|rt| {
            rt.get(r, "t").to_js_string().split_whitespace().count() >= 4
        });
        let t = rt.get(r, "t").to_js_string();
        (r, t.split_whitespace().map(str::to_string).collect())
    }

    /// Runs `helper` (an ignored test gated on `RI_TEST_HELPER`) in a child test process, so
    /// its process-wide effects (`terminate_all`, fatal signals) stay out of this binary.
    fn run_helper(helper: &str) -> std::process::Output {
        std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                &format!("native::tests::{helper}"),
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("RI_TEST_HELPER", helper)
            .output()
            .unwrap()
    }

    fn is_helper(helper: &str) -> bool {
        std::env::var("RI_TEST_HELPER").is_ok_and(|h| h == helper)
    }

    #[test]
    fn process_child_keeps_argv0_and_leads_its_group_terminated_at_shutdown() {
        // terminate_all() signals every Process of the binary, other tests' ones included.
        let out = run_helper("terminate_all_child_helper");
        assert!(
            out.status.success() && String::from_utf8_lossy(&out.stdout).contains("1 passed"),
            "{out:?}"
        );
    }

    /// Run by `process_child_keeps_argv0_and_leads_its_group_terminated_at_shutdown`.
    #[test]
    #[ignore = "child process: process_child_keeps_argv0_and_leads_its_group_terminated_at_shutdown runs it"]
    fn terminate_all_child_helper() {
        if !is_helper("terminate_all_child_helper") {
            return;
        }
        let mut rt = Runtime::new(None);
        let (_, v) = tree(&mut rt);
        let (sh, pgid, arg0, sleeper): (i32, i32, &str, i32) = (
            v[0].parse().unwrap(),
            v[1].parse().unwrap(),
            &v[2],
            v[3].parse().unwrap(),
        );
        // Same argv as the old setpriv/execvp path: argv[0] as written in `command`.
        assert_eq!(arg0, "sh");
        assert_eq!(pgid, sh, "the child leads its own process group");
        assert!(alive(sh) && alive(sleeper));
        super::children::terminate_all();
        // Neither exits on its own: a survivor would hang here.
        crate::testenv::wait_all_gone(&[sh, sleeper]);
        println!("TERMINATED");
    }

    /// Run by `process_tree_dies_with_the_shell_on_sigterm` in a child test process.
    #[test]
    #[ignore = "child process: process_tree_dies_with_the_shell_on_sigterm runs it"]
    fn sigterm_child_helper() {
        if !is_helper("sigterm_child_helper") {
            return;
        }
        let mut rt = Runtime::new(None);
        let (_, v) = tree(&mut rt);
        println!("\nPIDS {} {}", v[0], v[3]);
        use std::io::Write;
        let _ = std::io::stdout().flush();
        // SAFETY: raise(3) takes a plain signal number; ending this helper process is the point.
        unsafe { libc::raise(libc::SIGTERM) };
        // raise(3) returns after the handler ran, which re-raised the signal with its default
        // action: the process ends before this point.
        unreachable!("SIGTERM did not end the process");
    }

    #[test]
    fn process_tree_dies_with_the_shell_on_sigterm() {
        // What PR_SET_PDEATHSIG did for the child (and more: the grandchild too).
        let out = run_helper("sigterm_child_helper");
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(out.status.signal(), Some(libc::SIGTERM), "{out:?}");
        let text = String::from_utf8_lossy(&out.stdout);
        let pids: Vec<i32> = text
            .lines()
            .find_map(|l| l.split_once("PIDS ").map(|(_, p)| p))
            .expect("helper printed no pids")
            .split_whitespace()
            .map(|p| p.parse().unwrap())
            .collect();
        // The sleeper never exits on its own: one that outlived the shell would hang here.
        crate::testenv::wait_all_gone(&pids);
    }

    const LAUNCH_PATH_CHILD: &str = "RI_LAUNCH_PATH_CHILD";

    /// The release layout of tools/install.sh around a copy of this test binary: the releases
    /// R1 and R2, and the installed name `bin/t`, which `installed` makes lead to R1. Runs the
    /// test `name` as `bin/t` and returns that path and the path the child printed.
    fn own_window_of_the_installed_name(
        name: &str,
        installed: impl FnOnce(&std::path::Path),
    ) -> (std::path::PathBuf, std::path::PathBuf) {
        own_window_of_the_name(name, installed, |dir| {
            std::process::Command::new(dir.join("bin/t"))
        })
    }

    /// As [`own_window_of_the_installed_name`], the child started by the command `start`
    /// gives for the layout's directory, with the arguments that run the test `name` after it.
    fn own_window_of_the_name(
        name: &str,
        installed: impl FnOnce(&std::path::Path),
        start: impl FnOnce(&std::path::Path) -> std::process::Command,
    ) -> (std::path::PathBuf, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("ri-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let me = std::env::current_exe().unwrap();
        for r in ["R1", "R2"] {
            std::fs::create_dir_all(dir.join("releases").join(r)).unwrap();
            let t = dir.join("releases").join(r).join("t");
            if std::fs::hard_link(&me, &t).is_err() {
                std::fs::copy(&me, &t).unwrap();
            }
        }
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        // As the child's current directory names it.
        let dir = std::fs::canonicalize(&dir).unwrap();
        installed(&dir);
        let out = start(&dir)
            .args(["--exact", &format!("native::tests::{name}")])
            .args(["--nocapture", "--test-threads=1"])
            .env(LAUNCH_PATH_CHILD, &dir)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            out.status.success(),
            "{stdout}{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let shown = stdout
            .lines()
            .find_map(|l| l.split_once("own window: ").map(|(_, p)| p))
            .unwrap_or_else(|| panic!("the child said nothing:\n{stdout}"));
        (dir.join("bin/t"), shown.into())
    }

    /// In that child: once its launch path is fixed, an install makes `bin/t` the link to R2
    /// and, if `prune`, removes R1, the release this process runs; then the path its own
    /// windows start is printed. False in the parent.
    fn is_the_child_that_sees_an_install(prune: bool) -> bool {
        let Ok(dir) = std::env::var(LAUNCH_PATH_CHILD) else {
            return false;
        };
        let dir = std::path::Path::new(&dir);
        super::launch_path();
        std::fs::remove_file(dir.join("bin/t")).unwrap();
        std::os::unix::fs::symlink("../releases/R2/t", dir.join("bin/t")).unwrap();
        if prune {
            std::fs::remove_dir_all(dir.join("releases/R1")).unwrap();
        }
        println!("own window: {}", super::launch_path().display());
        true
    }

    /// The child started as `bin/t`, a link, sees an install replace its release and remove
    /// it, then starts its own windows by the path it was started as, which leads to the new
    /// release.
    #[test]
    fn own_windows_start_the_installed_release_after_an_install() {
        if is_the_child_that_sees_an_install(true) {
            return;
        }
        let (name, shown) = own_window_of_the_installed_name(
            "own_windows_start_the_installed_release_after_an_install",
            |dir| std::os::unix::fs::symlink("../releases/R1/t", dir.join("bin/t")).unwrap(),
        );
        assert_eq!(shown, name);
    }

    /// While tools/install.sh switches releases, the installed name leads to a script that
    /// starts a release under the path the script itself was started as. The install that
    /// follows keeps that release (it is the one to roll back to): the child's own windows
    /// still start by `bin/t`, the new release, not by the path of the binary it runs.
    #[test]
    fn own_windows_start_the_installed_release_after_a_launcher_started_this_one() {
        if is_the_child_that_sees_an_install(false) {
            return;
        }
        let (name, shown) = own_window_of_the_installed_name(
            "own_windows_start_the_installed_release_after_a_launcher_started_this_one",
            |dir| {
                use std::os::unix::fs::PermissionsExt;
                let script = format!(
                    "#!/bin/bash\nexec -a \"$0\" '{}' \"$@\"\n",
                    dir.join("releases/R1/t").display()
                );
                std::fs::write(dir.join("bin/t"), script).unwrap();
                let mode = std::fs::Permissions::from_mode(0o755);
                std::fs::set_permissions(dir.join("bin/t"), mode).unwrap();
            },
        );
        assert_eq!(shown, name);
    }

    /// In the child started in the layout's directory: once its launch path is fixed, it
    /// changes its directory, and the path its own windows start is printed. False in the
    /// parent.
    fn is_the_child_that_changes_its_directory() -> bool {
        if std::env::var_os(LAUNCH_PATH_CHILD).is_none() {
            return false;
        }
        super::launch_path();
        std::env::set_current_dir("/").unwrap();
        println!("own window: {}", super::launch_path().display());
        true
    }

    /// `sh` in the layout's directory, which runs `exec` there with the arguments after it.
    fn sh_in(dir: &std::path::Path, exec: &str) -> std::process::Command {
        let mut c = std::process::Command::new("/bin/sh");
        c.arg("-c")
            .arg(format!("cd \"$1\" && shift && {exec} \"$@\""))
            .arg("sh")
            .arg(dir);
        c
    }

    /// The child started as `bin/t`, a path relative to the directory it was started in: the
    /// path is made absolute once, at the start, so its own windows and its reload start the
    /// same file after the process changed its directory.
    #[test]
    fn a_relative_path_is_the_file_it_named_in_the_directory_the_process_started_in() {
        if is_the_child_that_changes_its_directory() {
            return;
        }
        let (name, shown) = own_window_of_the_name(
            "a_relative_path_is_the_file_it_named_in_the_directory_the_process_started_in",
            |dir| std::os::unix::fs::symlink("../releases/R1/t", dir.join("bin/t")).unwrap(),
            |dir| sh_in(dir, "exec bin/t"),
        );
        assert_eq!(shown, name);
    }

    /// The same for the child started as `t`, which a relative PATH entry (`bin`) leads to.
    #[test]
    fn a_name_on_a_relative_path_entry_is_the_file_in_the_directory_the_process_started_in() {
        if is_the_child_that_changes_its_directory() {
            return;
        }
        let (name, shown) = own_window_of_the_name(
            "a_name_on_a_relative_path_entry_is_the_file_in_the_directory_the_process_started_in",
            |dir| std::os::unix::fs::symlink("../releases/R1/t", dir.join("bin/t")).unwrap(),
            |dir| sh_in(dir, "export PATH=bin && exec t"),
        );
        assert_eq!(shown, name);
    }

    /// A command is run as written: `qs` is the program on PATH, not this binary.
    #[test]
    fn qs_command_is_run_as_written() {
        let argv = ["qs", "-p", "/x/settings.qml"].map(String::from);
        let launch = crate::detached::Launch::inherit(&argv, None);
        assert_eq!(launch.argv, argv);
        assert_eq!(launch.exe, super::resolve("qs", None));
        assert_ne!(launch.exe, std::env::current_exe().ok());
    }

    /// First run: welcome.qml is asked for during the shell's load and opens only once the
    /// bar has reserved its exclusive zone.
    #[test]
    fn app_window_waits_while_held() {
        let key = std::env::temp_dir().join(format!("qml-appwin-held-{}.qml", std::process::id()));
        let (tx, rx) = std::sync::mpsc::channel();
        super::hold_app_windows();
        assert!(super::app_windows_held());
        super::open_app_window(
            key.clone(),
            move || {
                tx.send(()).unwrap();
                None
            },
            |_| {},
        );
        // Held: queued on this thread, so nothing that could run it reached the pool.
        assert_eq!(
            super::HELD_APP_WINDOWS.with_borrow(|h| h.as_ref().map(Vec::len)),
            Some(1)
        );
        assert!(rx.try_recv().is_err());
        super::release_app_windows();
        assert!(!super::app_windows_held());
        rx.recv().unwrap();
    }

    #[test]
    fn app_window_opens_once_and_focuses_while_running() {
        let key = std::env::temp_dir().join(format!("qml-appwin-{}.qml", std::process::id()));

        let (tx, rx) = std::sync::mpsc::channel::<(&'static str, u32)>();
        // A `gate` holds the spawn back until the test opens it, so a click lands while the
        // process starts however the threads are scheduled.
        let open = |tx: std::sync::mpsc::Sender<(&'static str, u32)>,
                    gate: Option<std::sync::mpsc::Receiver<()>>| {
            let t2 = tx.clone();
            super::open_app_window(
                key.clone(),
                move || {
                    if let Some(gate) = gate {
                        gate.recv().unwrap();
                    }
                    let ch = std::process::Command::new("sleep")
                        .arg("infinity")
                        .spawn()
                        .ok()?;
                    tx.send(("spawn", ch.id())).unwrap();
                    Some(ch.id())
                },
                move |pid| t2.send(("focus", pid)).unwrap(),
            )
        };
        let wait = |rx: &std::sync::mpsc::Receiver<(&'static str, u32)>| rx.recv().unwrap();
        // Two quick clicks: one process; the second click lands while it starts and opens nothing.
        let (go, gate) = std::sync::mpsc::channel();
        open(tx.clone(), Some(gate));
        open(tx.clone(), None);
        go.send(()).unwrap();
        let (what, pid) = wait(&rx);
        assert_eq!(what, "spawn");
        super::wait_app_windows(|apps| !apps.iter().any(|(k, p)| *k == key && *p == 0));
        // A click while it runs focuses it.
        open(tx.clone(), None);
        assert_eq!(wait(&rx), ("focus", pid));
        // Closed: the next click opens a new one.
        // SAFETY: kill(2) takes plain integers and touches no memory of this process.
        unsafe { libc::kill(pid as i32, libc::SIGTERM) };
        super::wait_app_windows(|apps| !apps.iter().any(|(k, _)| *k == key));
        open(tx.clone(), None);
        let (what, pid2) = wait(&rx);
        assert_eq!(what, "spawn");
        assert!(rx.try_recv().is_err());
        // SAFETY: kill(2) takes plain integers and touches no memory of this process.
        unsafe { libc::kill(pid2 as i32, libc::SIGTERM) };
    }

    #[test]
    fn process_resolves_program_like_execvp() {
        // PATH from `environment` first, then the shell's; a missing program fails to start
        // (no started/exited), as QProcess::FailedToStart.
        let dir = std::env::temp_dir().join(format!("ri-proc-path-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let tool = dir.join("ri-proc-tool");
        crate::testenv::install_script(&tool, "#!/bin/sh\necho \"$0:$1\"\n");
        let mut rt = Runtime::new(None);
        let r = rt
            .load_str(&format!(
                "import QtQuick\nimport Rustshell.Io\nItem {{ id: r; property string t; property int code: -9; property int bad: 0\n\
                 Process {{ running: true; command: ['ri-proc-tool', 'x']; environment: ({{ PATH: '{}:/usr/bin:/bin' }})\n\
                   stdout: StdioCollector {{ onStreamFinished: r.t = text }}\n\
                   onExited: (c) => r.code = c }}\n\
                 Process {{ id: m; running: true; command: ['ri-no-such-program-xyz']\n\
                   onStarted: r.bad++; onExited: r.bad++ }} }}",
                dir.display()
            ))
            .unwrap();
        rt.settle();
        run_until(&mut rt, &|rt| rt.get(r, "code").to_number() == 0.0);
        assert_eq!(
            rt.get(r, "t").to_js_string().trim(),
            format!("{}:x", tool.display())
        );
        let m = rt.id("m").unwrap();
        assert!(!rt.get(m, "running").truthy());
        assert_eq!(rt.get(r, "bad").to_number(), 0.0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn process_started_handler_ending_in_postfix_increment_keeps_later_handlers() {
        // Warp's connectProc: a handler line ending in `n++` is a complete statement (ASI), so
        // the following `onRunningChanged`/`onExited` members are handlers, not JS labels that
        // run inside `onStarted`.
        let mut rt = Runtime::new(None);
        let r = rt
            .load_str(
                "import QtQuick\nimport Rustshell.Io\nItem { id: root; property int code: 0; property int n: 0; property int exits: 0; property var log: []\n\
                 Process { id: p; command: ['bash', '-c', 'exit ' + root.code]\n\
                   onStarted: root.n++\n\
                   onRunningChanged: root.log = root.log.concat([running])\n\
                   onExited: (c, s) => { root.exits++ } }\n\
                 function go() { root.code = 1; p.running = true } }",
            )
            .unwrap();
        let p = rt.id("p").unwrap();
        rt.settle();
        assert!(rt.call_method(r, "go", vec![]).is_some());
        run_until(&mut rt, &|rt| rt.get(r, "exits").to_number() == 1.0);
        assert_eq!(rt.get(r, "n").to_number(), 1.0);
        assert!(!rt.get(p, "running").truthy());
        assert_eq!(rt.get(r, "log").to_js_string(), "true,false");
    }

    /// Destroying a running Process (its Loader unloads) ends its whole group, not only the
    /// leader: the backgrounded sleeper and `wait`ing shell both go, and its shutdown slot is
    /// freed (a stale slot would SIGTERM a reused group id when the shell exits).
    #[test]
    fn dropped_runtime_kills_its_process_children_and_frees_their_slots() {
        // The shell's exit drops the World before `terminate_children`, and so does every
        // Runtime a test or `qs -p` window drops: each running child is SIGKILLed (as
        // QProcess::~QProcess) and leaves its shutdown slot before the reaper may reap it.
        let mut rt = Runtime::new(None);
        rt.load_str(
            "import QtQuick\nimport Rustshell.Io\nItem { Process { id: p; running: true; command: ['sleep', '300'] } }",
        )
        .unwrap();
        rt.settle();
        let p = rt.id("p").unwrap();
        let pid = rt.get(p, "processId").to_number() as i32;
        assert!(pid > 0, "the Process did not start");
        assert!(super::children::tracked(pid));
        let fd = workpool::pidfd_open(pid).unwrap();
        drop(rt);
        assert!(!super::children::tracked(pid), "shutdown slot leaked");
        let mut pfd = libc::pollfd {
            fd: std::os::fd::AsRawFd::as_raw_fd(&fd),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: `pfd` is one live pollfd over an open pidfd.
        while unsafe { libc::poll(&mut pfd, 1, -1) } < 0 {
            assert_eq!(
                std::io::Error::last_os_error().kind(),
                std::io::ErrorKind::Interrupted
            );
        }
    }

    #[test]
    fn destroyed_process_ends_its_group_and_frees_its_slot() {
        let mut rt = Runtime::new(None);
        let r = rt
            .load_str(
                "import QtQuick\nimport Rustshell.Io\nItem { id: r; property string t; property bool on: true\n\
                 Loader { active: r.on; sourceComponent: Component { Process { running: true; command: ['sh', '-c', 'echo $$; sleep infinity & echo $!; wait']\n\
                   stdout: SplitParser { onRead: data => r.t = r.t + data + ' ' } } } } }",
            )
            .unwrap();
        rt.settle();
        run_until(&mut rt, &|rt| {
            rt.get(r, "t").to_js_string().split_whitespace().count() >= 2
        });
        let v: Vec<i32> = rt
            .get(r, "t")
            .to_js_string()
            .split_whitespace()
            .map(|p| p.parse().unwrap())
            .collect();
        let (sh, sleeper) = (v[0], v[1]);
        assert!(super::children::tracked(sh));
        rt.set(r, "on", Value::Bool(false));
        rt.settle();
        // Neither exits on its own: a survivor would hang here.
        crate::testenv::wait_all_gone(&[sh, sleeper]);
        rt.poll_native();
        assert!(!super::children::tracked(sh), "shutdown slot leaked");
    }

    #[test]
    fn process_keeps_running_until_terminated_child_exits() {
        // Quickshell Process::setRunning(false) only terminate()s: a child that ignores SIGTERM
        // (cava 1.0 hangs in its handler) stays the running process, `running = true` does not
        // start a second one, and the restart waits for the exit. Each child reports once its
        // trap is set, reports each SIGTERM its trap handled, and exits only once the test writes
        // the FIFO `go`, so no step races its timing. A `wait` the trap interrupted is waited
        // again while `head` lives (its status may be gone once it was reaped meanwhile).
        let dir = std::env::temp_dir().join(format!("ri-term-wait-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let go = dir.join("go");
        crate::testenv::fifo(&go);
        let mut rt = Runtime::new(None);
        let r = rt
            .load_str(&format!(
                "import QtQuick\nimport Rustshell.Io\nItem {{ id: r; property bool on: true; property int starts; property int exits; property int ready; property int terms\n\
                 Process {{ id: p; running: r.on; command: ['sh', '-c', 'trap \"echo term\" TERM; echo ready; head -n1 \"$0\" >/dev/null & h=$!; wait $h; while kill -0 $h 2>/dev/null; do wait $h; done', '{}']\n\
                   stdout: SplitParser {{ onRead: data => data === 'term' ? r.terms++ : r.ready++ }}\n\
                   onStarted: r.starts++; onExited: r.exits++ }} }}",
                go.display()
            ))
            .unwrap();
        let p = rt.id("p").unwrap();
        let num = |rt: &Runtime, k: &str| rt.get(r, k).to_number();
        rt.settle();
        run_until(&mut rt, &|rt| num(rt, "ready") == 1.0);
        assert_eq!(num(&rt, "starts"), 1.0);
        let pid = rt.get(p, "processId").to_number();
        rt.set(r, "on", Value::Bool(false));
        rt.poll_native();
        assert!(rt.get(p, "running").truthy());
        rt.set(r, "on", Value::Bool(true));
        rt.poll_native();
        assert_eq!(num(&rt, "starts"), 1.0);
        assert_eq!(rt.get(p, "processId").to_number(), pid);
        run_until(&mut rt, &|rt| num(rt, "terms") == 1.0);
        assert_eq!(
            num(&rt, "exits"),
            0.0,
            "the SIGTERMed child exited on its own"
        );
        std::fs::write(&go, "\n").unwrap();
        run_until(&mut rt, &|rt| num(rt, "ready") == 2.0);
        assert_eq!(num(&rt, "starts"), 2.0);
        assert_eq!(num(&rt, "exits"), 1.0);
        assert_ne!(rt.get(p, "processId").to_number(), pid);
        rt.set(r, "on", Value::Bool(false));
        rt.poll_native();
        assert!(rt.get(p, "running").truthy());
        std::fs::write(&go, "\n").unwrap();
        run_until(&mut rt, &|rt| num(rt, "exits") == 2.0);
        assert!(!rt.get(p, "running").truthy());
        assert_eq!(num(&rt, "starts"), 2.0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Blocks until the process behind `fd` (a pidfd) exited.
    fn wait_gone(fd: &std::os::fd::OwnedFd) {
        let mut pfd = libc::pollfd {
            fd: std::os::fd::AsRawFd::as_raw_fd(fd),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: `pfd` is one live pollfd over an open pidfd.
        while unsafe { libc::poll(&mut pfd, 1, -1) } < 0 {
            assert_eq!(
                std::io::Error::last_os_error().kind(),
                std::io::ErrorKind::Interrupted
            );
        }
    }

    /// Exit status and code of a Process run to its end: `exits`, `code`, `status`, `t` (the
    /// last stdout line).
    const EXIT_PROBE: &str = "import QtQuick\nimport Rustshell.Io\nItem { id: r; property int exits; property var code: -1; property var status: -1; property string t\n\
        property alias cmd: p.command\n\
        Process { id: p; stdout: SplitParser { onRead: d => r.t = d }\n\
          onExited: (c, s) => { r.exits++; r.code = c; r.status = s } } }";

    #[test]
    fn a8_exit_is_reported_when_the_child_dies_not_at_pipe_eof() {
        // QProcess finishes when its child dies (`_q_processDied`): what the pipes hold is
        // read, then they are closed, though a backgrounded descendant still has them open.
        let mut rt = Runtime::new(None);
        let r = rt.load_str(EXIT_PROBE).unwrap();
        let p = rt.id("p").unwrap();
        rt.settle();
        let cmd = |n: &str| {
            Value::array(
                ["sh", "-c", &format!("sleep infinity & echo $!; exit {n}")]
                    .iter()
                    .map(|s| Value::str(s))
                    .collect(),
            )
        };
        let mut sleepers = vec![];
        for (n, code) in [(1.0, 3.0), (2.0, 4.0)] {
            rt.set(r, "cmd", cmd(&format!("{code}")));
            rt.set(p, "running", Value::Bool(true));
            run_until(&mut rt, &|rt| rt.get(r, "exits").to_number() == n);
            assert_eq!(rt.get(r, "code").to_number(), code);
            assert_eq!(rt.get(r, "status").to_number(), 0.0);
            assert!(!rt.get(p, "running").truthy());
            let pid: i32 = rt.get(r, "t").to_js_string().trim().parse().unwrap();
            sleepers.push(workpool::pidfd_open(pid).unwrap());
            // SAFETY: kill(2) takes plain integers; the sleeper is alive (its pidfd is open).
            unsafe { libc::kill(pid, libc::SIGKILL) };
        }
        for fd in &sleepers {
            wait_gone(fd);
        }
    }

    #[test]
    fn a8_signalled_child_reports_signal_and_crash_exit() {
        // QProcess: a child ended by a signal is CrashExit with exitCode = WTERMSIG.
        let mut rt = Runtime::new(None);
        let r = rt.load_str(EXIT_PROBE).unwrap();
        let p = rt.id("p").unwrap();
        rt.settle();
        rt.set(
            r,
            "cmd",
            Value::array(vec![
                Value::str("sh"),
                Value::str("-c"),
                Value::str("kill -9 $$"),
            ]),
        );
        rt.set(p, "running", Value::Bool(true));
        run_until(&mut rt, &|rt| rt.get(r, "exits").to_number() == 1.0);
        assert_eq!(
            (
                rt.get(r, "code").to_number(),
                rt.get(r, "status").to_number()
            ),
            (9.0, 1.0)
        );
    }

    #[test]
    fn a8_process_whose_pipes_cannot_be_watched_fails_to_start() {
        // The World's epoll set refusing the pipes (ENOMEM, ENOSPC; here: not an epoll fd):
        // nobody would read them, so the child is ended and the Process does not run.
        let mut rt = Runtime::new(None);
        let r = rt.load_str(EXIT_PROBE).unwrap();
        let p = rt.id("p").unwrap();
        rt.settle();
        {
            use std::os::fd::FromRawFd;
            // SAFETY: eventfd(2) takes plain integers; the new fd is owned by nothing else.
            let fd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC) };
            assert!(fd >= 0);
            // SAFETY: `fd` was just created and is owned by nothing else.
            rt.world.borrow_mut().proc_ep = Some(unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) });
        }
        rt.set(
            r,
            "cmd",
            Value::array(vec![Value::str("sleep"), Value::str("infinity")]),
        );
        rt.set(p, "running", Value::Bool(true));
        assert!(!rt.get(p, "running").truthy());
        assert!(matches!(
            rt.get(p, "processId"),
            Value::Null | Value::Undefined
        ));
    }

    /// An in-process job running a program that ignores SIGTERM, started as Process `p`; the
    /// program's pidfd once it ignores TERM.
    fn start_term_ignoring_job(rt: &mut Runtime, p: usize) -> std::os::fd::OwnedFd {
        let (pids, pid) = std::sync::mpsc::channel();
        let job: crate::jobs::Job = Box::new(move |_io, _stop| {
            use std::io::BufRead;
            let Ok(mut g) = crate::child::spawn(
                std::process::Command::new("sh")
                    .args(["-c", "trap '' TERM; echo up; exec sleep infinity"])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::piped()),
            ) else {
                return 127;
            };
            let mut up = String::new();
            let o = g.child_mut().stdout.take().unwrap();
            let _ = std::io::BufReader::new(o).read_line(&mut up);
            let _ = pids.send(g.id() as i32);
            g.wait().ok().and_then(|s| s.code()).unwrap_or(1)
        });
        {
            let mut w = rt.world.borrow_mut();
            let (tx, rx) = w.waker.handle(crate::svc::Svc::Process).channel();
            let stop = std::sync::Arc::new(crate::svc::Stop::default());
            crate::jobs::start(job, tx, stop.clone());
            w.procs.insert(
                p,
                super::Proc {
                    pid: 0,
                    stop: Some(stop),
                    queued: Default::default(),
                    job: Some(rx),
                    io: None,
                    out: String::new(),
                    err: String::new(),
                    restart: false,
                    stdin: None,
                    after_frame: None,
                },
            );
            w.write(p, "running", Value::Bool(true));
        }
        workpool::pidfd_open(pid.recv().unwrap()).unwrap()
    }

    #[test]
    fn a8_dropping_the_runtime_kills_a_job_child_that_handles_sigterm() {
        // ~QProcess kills (SIGKILL) what still runs; an in-process job's programs alike.
        let mut rt = Runtime::new(None);
        rt.load_str(
            "import QtQuick\nimport Rustshell.Io\nItem { Process { id: p; command: ['x'] } }",
        )
        .unwrap();
        rt.settle();
        let p = rt.id("p").unwrap();
        let child = start_term_ignoring_job(&mut rt, p);
        drop(rt);
        wait_gone(&child);
    }

    #[test]
    fn a8_destroyed_process_kills_its_job_child_that_handles_sigterm() {
        let mut rt = Runtime::new(None);
        let r = rt
            .load_str(
                "import QtQuick\nimport Rustshell.Io\nItem { id: r; property bool on: true\n\
                 Loader { active: r.on; sourceComponent: Component { Process { objectName: 'p'; command: ['x'] } } } }",
            )
            .unwrap();
        rt.settle();
        let p = {
            let w = rt.world.borrow();
            (0..w.items.len())
                .find(|&i| w.items[i].types.iter().any(|t| t == "Process"))
                .unwrap()
        };
        let child = start_term_ignoring_job(&mut rt, p);
        rt.set(r, "on", Value::Bool(false));
        rt.settle();
        rt.poll_native();
        wait_gone(&child);
    }

    #[test]
    fn a8_terminated_job_child_is_still_killed_with_the_runtime() {
        // `running = false` is QProcess::terminate (SIGTERM, which this child ignores); the
        // Runtime's drop then kills it as ~QProcess would.
        let mut rt = Runtime::new(None);
        let r = rt.load_str(EXIT_PROBE).unwrap();
        let p = rt.id("p").unwrap();
        rt.settle();
        let child = start_term_ignoring_job(&mut rt, p);
        rt.poll_native();
        rt.set(p, "running", Value::Bool(false));
        run_until(&mut rt, &|rt| rt.get(r, "exits").to_number() == 1.0);
        assert_eq!(
            (
                rt.get(r, "code").to_number(),
                rt.get(r, "status").to_number()
            ),
            (15.0, 1.0)
        );
        drop(rt);
        wait_gone(&child);
    }

    #[test]
    fn image_job_output_waits_for_next_frame() {
        // An image job finishes before its window's next frame; the real script never does,
        // and the background widgets center on its result with their laid-out `width / 2`.
        let mut rt = Runtime::new(None);
        let win = rt
            .load_str(
                "import QtQuick\nimport Quickshell\nimport Rustshell.Io\nPanelWindow { id: win; property int exits\n\
                 Process { id: p; command: ['true']; onExited: win.exits++ } }",
            )
            .unwrap();
        let p = rt.id("p").unwrap();
        rt.settle();
        let (tx, rx) = std::sync::mpsc::channel();
        {
            let mut w = rt.world.borrow_mut();
            w.procs.insert(
                p,
                super::Proc {
                    pid: 0,
                    stop: None,
                    queued: Default::default(),
                    job: Some(rx),
                    io: None,
                    out: String::new(),
                    err: String::new(),
                    restart: false,
                    stdin: None,
                    after_frame: Some(0),
                },
            );
            w.write(p, "running", Value::Bool(true));
        }
        tx.send(super::ProcEv::Exit(0)).unwrap();
        // The host has not reported its windows yet, then has but not drawn this one.
        rt.poll_native();
        assert_eq!(rt.get(win, "exits").to_number(), 0.0);
        assert!(rt.set_frame_wins([win]));
        rt.poll_native();
        assert_eq!(rt.get(win, "exits").to_number(), 0.0);
        assert!(rt.get(p, "running").truthy());
        assert!(rt.world.borrow().frame_wanted.contains(&win));
        rt.world.borrow_mut().win_frames.insert(win, 1);
        rt.world.borrow_mut().revision += 1;
        rt.poll_native();
        assert_eq!(rt.get(win, "exits").to_number(), 1.0);
        assert!(!rt.get(p, "running").truthy());
    }

    /// A Process `p` in `qml` running an in-process image job started at frame 0 whose output
    /// (its exit) is already there.
    fn finished_image_job(qml: &str) -> (Runtime, usize) {
        let mut rt = Runtime::new(None);
        rt.load_str(qml).unwrap();
        let p = rt.id("p").unwrap();
        rt.settle();
        let (tx, rx) = std::sync::mpsc::channel();
        {
            let mut w = rt.world.borrow_mut();
            w.frame_paced = true;
            w.procs.insert(
                p,
                super::Proc {
                    pid: 0,
                    stop: None,
                    queued: Default::default(),
                    job: Some(rx),
                    io: None,
                    out: String::new(),
                    err: String::new(),
                    restart: false,
                    stdin: None,
                    after_frame: Some(0),
                },
            );
            w.write(p, "running", Value::Bool(true));
        }
        tx.send(super::ProcEv::Exit(0)).unwrap();
        (rt, p)
    }

    #[test]
    fn image_job_in_a_window_that_cannot_draw_reports_at_once() {
        // A hidden (or unconfigured) window draws no frame to wait for.
        let (mut rt, p) = finished_image_job(
            "import QtQuick\nimport Quickshell\nimport Rustshell.Io\nPanelWindow { id: win\n\
             Process { id: p; command: ['true'] } }",
        );
        rt.poll_native();
        assert!(rt.get(p, "running").truthy());
        assert!(rt.set_frame_wins([]));
        rt.poll_native();
        assert!(!rt.get(p, "running").truthy());
    }

    #[test]
    fn windowless_image_job_waits_for_any_frame_while_a_window_draws() {
        let (mut rt, p) = finished_image_job(
            "import QtQuick\nimport Rustshell.Io\nItem { Process { id: p; command: ['true'] } }",
        );
        rt.poll_native();
        assert!(rt.get(p, "running").truthy());
        assert!(rt.set_frame_wins([usize::MAX - 1]));
        rt.poll_native();
        assert!(rt.get(p, "running").truthy());
        assert!(rt.world.borrow().frame_wanted.contains(&usize::MAX));
        // The window is hidden before it drew: nothing will draw, the output goes through.
        assert!(rt.set_frame_wins([]));
        rt.poll_native();
        assert!(!rt.get(p, "running").truthy());
    }

    #[test]
    fn in_process_job_restarts_on_running_false_true() {
        // Translator.qml's debounce: `running = false; buffer = ""; running = true` while the
        // translateProc job is in flight. Quickshell terminate()s it (exited 15), then starts the
        // command again; the in-process job must not keep running in place of the new one.
        let mut rt = Runtime::new(None);
        let r = rt
            .load_str(
                "import QtQuick\nimport Rustshell.Io\nItem { id: r; property bool go; property int starts; property int exits\n\
                 property var code: -1; property string out\n\
                 onGoChanged: { p.running = false; r.out = ''; p.running = true }\n\
                 Process { id: p; command: ['sh', '-c', 'echo fresh']\n\
                   stdout: SplitParser { onRead: d => r.out += d }\n\
                   onStarted: r.starts++; onExited: (c, s) => { r.exits++; if (r.code < 0) r.code = c } } }",
            )
            .unwrap();
        let p = rt.id("p").unwrap();
        rt.settle();
        let stop = std::sync::Arc::new(crate::svc::Stop::default());
        let (_tx, rx) = std::sync::mpsc::channel();
        {
            let mut w = rt.world.borrow_mut();
            w.procs.insert(
                p,
                super::Proc {
                    pid: 0,
                    stop: Some(stop.clone()),
                    queued: Default::default(),
                    job: Some(rx),
                    io: None,
                    out: String::new(),
                    err: String::new(),
                    restart: false,
                    stdin: None,
                    after_frame: None,
                },
            );
            w.write(p, "running", Value::Bool(true));
        }
        rt.poll_native();
        rt.set(r, "go", Value::Bool(true));
        assert!(stop.load(std::sync::atomic::Ordering::Relaxed));
        run_until(&mut rt, &|rt| rt.get(r, "exits").to_number() == 2.0);
        assert_eq!(rt.get(r, "starts").to_number(), 1.0);
        assert_eq!(rt.get(r, "exits").to_number(), 2.0);
        assert_eq!(rt.get(r, "code").to_number(), 15.0);
        assert_eq!(rt.get(r, "out").to_js_string(), "fresh");
        assert!(!rt.get(p, "running").truthy());
    }

    #[test]
    fn process_runs_command_bound_when_started() {
        // `arg = e; running = true; arg = ""` runs the command with `e`.
        let mut rt = Runtime::new(None);
        let r = rt
            .load_str(
                "import QtQuick\nimport Rustshell.Io\nItem { id: r; property string t: 'none'\n\
                 Process { id: p; property string arg; command: ['printf', '%s', 'got:' + arg]\n\
                   stdout: StdioCollector { onStreamFinished: r.t = text } }\n\
                 Component.onCompleted: { p.arg = 'x'; p.running = true; p.arg = '' } }",
            )
            .unwrap();
        rt.settle();
        run_until(&mut rt, &|rt| rt.get(r, "t").to_js_string() != "none");
        assert_eq!(rt.get(r, "t").to_js_string(), "got:x");
    }

    /// What a program started detached reports about itself into the FIFO `$0`: its
    /// `/proc/self/stat`, working directory, the variables `FOO` and `HOME`, and whether it was
    /// started with a `PATH` (from `/proc/self/environ`: a shell started without one sets its
    /// own). The shell's builtins only, so it runs with an empty environment too.
    pub(crate) const REPORT: &str = "read -r stat < /proc/self/stat; read -r env < /proc/self/environ; \
        case \"$env\" in *PATH=*) path=set ;; *) path= ;; esac; \
        printf '%s\\n%s\\n%s\\n%s\\n%s\\n' \"$stat\" \"$PWD\" \"${FOO-unset}\" \"${HOME-unset}\" \
        \"$path\" > \"$0\"";

    /// A [`REPORT`] read from its FIFO (blocks until the program wrote it).
    pub(crate) struct Report {
        pub pid: i32,
        pub pgrp: i32,
        pub session: i32,
        pub cwd: String,
        pub foo: String,
        pub home: String,
        /// Whether the program was started with a `PATH`.
        pub path: bool,
    }

    pub(crate) fn report(fifo: &std::path::Path) -> Report {
        let text = std::fs::read_to_string(fifo).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 5, "{text:?}");
        // pid (comm) state ppid pgrp session ...
        let rest: Vec<&str> = lines[0]
            .rsplit_once(')')
            .unwrap()
            .1
            .split_whitespace()
            .collect();
        Report {
            pid: lines[0].split_whitespace().next().unwrap().parse().unwrap(),
            pgrp: rest[2].parse().unwrap(),
            session: rest[3].parse().unwrap(),
            cwd: lines[1].into(),
            foo: lines[2].into(),
            home: lines[3].into(),
            path: lines[4] == "set",
        }
    }

    /// A FIFO in a new directory.
    pub(crate) fn report_fifo() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("report");
        crate::testenv::fifo(&f);
        (dir, f)
    }

    /// Detached as `QProcess::startDetached`: the program leads a session of its own (and so
    /// its own group), where the shell's terminal and a signal to the shell's group do not
    /// reach it.
    #[track_caller]
    pub(crate) fn assert_detached(r: &Report) {
        assert_eq!(r.session, r.pid, "own session");
        assert_eq!(r.pgrp, r.pid, "own process group");
        // SAFETY: getsid(2) takes a plain integer and touches no memory of this process.
        assert_ne!(r.session, unsafe { libc::getsid(0) }, "the shell's session");
    }

    /// Runs `f` with this process's stderr going to a file; what was written there.
    fn stderr_of(f: impl FnOnce()) -> String {
        use std::io::{Read as _, Seek as _};
        use std::os::fd::AsRawFd as _;
        let mut log = tempfile::tempfile().unwrap();
        // SAFETY: dup(2) of this process's open stderr.
        let stderr = unsafe { libc::dup(2) };
        assert!(stderr >= 0);
        // SAFETY: dup2(2) between two open descriptors; `log` stays open for the call.
        assert_eq!(unsafe { libc::dup2(log.as_raw_fd(), 2) }, 2);
        f();
        // SAFETY: dup2(2) between two open descriptors, putting stderr back.
        assert_eq!(unsafe { libc::dup2(stderr, 2) }, 2);
        // SAFETY: close(2) of the copy made above, used by nothing else.
        unsafe { libc::close(stderr) };
        let mut text = String::new();
        log.rewind().unwrap();
        log.read_to_string(&mut text).unwrap();
        text
    }

    /// `detach` (Rustshell.execDetached) returns before the child starts, puts it in its own
    /// session and process group, and leaves no zombie behind.
    #[test]
    fn detach_neither_blocks_nor_leaves_a_zombie() {
        let (dir, f) = report_fifo();
        let sh = |script: &str, arg: &std::path::Path| {
            let argv = ["sh", "-c", script, &arg.to_string_lossy()].map(String::from);
            crate::detached::Launch::inherit(&argv, None)
        };
        super::detach("test", sh(REPORT, &f));
        // Runs until the test opens and closes the FIFO it reads: a detach that waited for it
        // would never return.
        let held = dir.path().join("held");
        crate::testenv::fifo(&held);
        super::detach("test", sh("read -r _ < \"$0\"", &held));
        let r = report(&f);
        assert_detached(&r);
        let pid = r.pid;
        drop(std::fs::OpenOptions::new().write(true).open(&held).unwrap());
        // Both exited, and the pool's reaper has waited for every child it watches.
        workpool::wait_idle();
        assert!(
            !std::path::Path::new(&format!("/proc/{pid}")).exists(),
            "child {pid} not reaped"
        );
    }

    /// `clearEnvironment: true` clears first and then applies `environment` (Quickshell's
    /// `setupProcessEnvironment`): the listed variables are the program's whole environment, a
    /// `null` one takes the shell's value, and nothing else is passed on. The clearing used to
    /// come last and removed them all.
    #[test]
    fn process_clear_environment_keeps_the_listed_variables() {
        let home = std::env::var("HOME").expect("HOME is set");
        assert!(std::env::var_os("PATH").is_some());
        let mut rt = Runtime::new(None);
        let r = rt
            .load_str(
                "import QtQuick\nimport Rustshell.Io\nItem { id: r; property string t: 'none'\n\
                 Process { running: true; clearEnvironment: true\n\
                   environment: ({ FOO: 'bar', HOME: null, UNSET_HERE: null, NOPE: undefined })\n\
                   command: ['/bin/sh', '-c', 'read -r env < /proc/self/environ; case \"$env\" in *PATH=*) path=set ;; esac; echo \"${FOO-unset}|${HOME-unset}|${UNSET_HERE-unset}|${NOPE-unset}|$path\"']\n\
                   stdout: StdioCollector { onStreamFinished: r.t = text } } }",
            )
            .unwrap();
        rt.settle();
        run_until(&mut rt, &|rt| rt.get(r, "t").to_js_string() != "none");
        assert_eq!(
            rt.get(r, "t").to_js_string().trim_end(),
            format!("bar|{home}|unset|unset|")
        );
    }

    /// `Rustshell.execDetached(context)` applies the context's `environment`,
    /// `clearEnvironment` and `workingDirectory`, and the program starts detached.
    #[test]
    fn exec_detached_applies_the_context_and_starts_a_session() {
        let home = std::env::var("HOME").expect("HOME is set");
        let (dir, f) = report_fifo();
        let cwd = dir.path().canonicalize().unwrap();
        let mut rt = Runtime::new(None);
        // Cleared: only FOO and the shell's HOME (null).
        rt.load_str(&format!(
            "import QtQuick\nimport Quickshell\nItem {{ Component.onCompleted: Rustshell.execDetached({{\n\
               command: ['/bin/sh', '-c', {REPORT:?}, {f:?}], clearEnvironment: true,\n\
               environment: {{ FOO: 'bar', HOME: null }}, workingDirectory: {cwd:?} }}) }}"
        ))
        .unwrap();
        rt.settle();
        let r = report(&f);
        assert_detached(&r);
        assert_eq!(
            (r.cwd.as_str(), r.foo.as_str(), r.home.as_str(), r.path),
            (cwd.to_str().unwrap(), "bar", home.as_str(), false)
        );
        // Not cleared: the shell's environment with FOO set and HOME (null) removed.
        rt.load_str(&format!(
            "import QtQuick\nimport Quickshell\nItem {{ Component.onCompleted: Rustshell.execDetached({{\n\
               command: ['sh', '-c', {REPORT:?}, {f:?}], environment: {{ FOO: 'baz', HOME: null }} }}) }}"
        ))
        .unwrap();
        rt.settle();
        let r = report(&f);
        assert_detached(&r);
        assert_eq!((r.foo.as_str(), r.home.as_str()), ("baz", "unset"));
        // A plain command list still runs.
        rt.load_str(&format!(
            "import QtQuick\nimport Quickshell\nItem {{ Component.onCompleted: Rustshell.execDetached(['sh', '-c', {REPORT:?}, {f:?}]) }}"
        ))
        .unwrap();
        rt.settle();
        let r = report(&f);
        assert_detached(&r);
        assert_eq!((r.foo.as_str(), r.home.as_str()), ("unset", home.as_str()));
        workpool::wait_idle();
    }

    /// `Process.startDetached()` starts the Process's command with its `environment`,
    /// `clearEnvironment` and `workingDirectory` (Quickshell's `Process::startDetached`), not
    /// the command alone.
    #[test]
    fn process_start_detached_applies_its_properties_and_starts_a_session() {
        let (dir, f) = report_fifo();
        let cwd = dir.path().canonicalize().unwrap();
        let mut rt = Runtime::new(None);
        rt.load_str(&format!(
            "import QtQuick\nimport Rustshell.Io\nItem {{\n\
             Process {{ id: p; command: ['/bin/sh', '-c', {REPORT:?}, {f:?}]; clearEnvironment: true\n\
               environment: ({{ FOO: 'bar' }}); workingDirectory: {cwd:?} }}\n\
             Component.onCompleted: p.startDetached() }}"
        ))
        .unwrap();
        rt.settle();
        let r = report(&f);
        assert_detached(&r);
        assert_eq!(
            (r.cwd.as_str(), r.foo.as_str(), r.home.as_str(), r.path),
            (cwd.to_str().unwrap(), "bar", "unset", false)
        );
        workpool::wait_idle();
    }

    /// A detached program that cannot start is reported with its name, as QProcess reports
    /// `FailedToStart`: one that does not exist, one that is not executable, and a working
    /// directory that does not exist.
    #[test]
    fn a_failed_detached_launch_is_logged_with_the_program() {
        const TEST: &str = concat!(
            module_path!(),
            "::a_failed_detached_launch_is_logged_with_the_program"
        );
        // In a process of its own: the test reads what that process wrote to its stderr.
        if !crate::testenv::in_own_process(TEST) {
            // No `xdg-open` there: PATH names an empty directory.
            let empty = tempfile::tempdir().unwrap();
            crate::testenv::run_in_own_process(
                TEST,
                [
                    ("LC_ALL", std::ffi::OsStr::new("C")),
                    ("PATH", empty.path().as_os_str()),
                ],
            );
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let plain = dir.path().join("ri-not-executable");
        std::fs::write(&plain, "text").unwrap();
        let text = stderr_of(|| {
            let mut rt = Runtime::new(None);
            rt.load_str(&format!(
                "import QtQuick\nimport Quickshell\nimport Rustshell.Io\nItem {{\n\
                 Process {{ id: p; command: ['ri-no-such-process-program', 'x'] }}\n\
                 Component.onCompleted: {{\n\
                   Rustshell.execDetached(['ri-no-such-program', 'arg']);\n\
                   Rustshell.execDetached([{plain:?}]);\n\
                   Rustshell.execDetached({{ command: ['/bin/sh', '-c', 'true'], workingDirectory: '/ri-no-such-dir' }});\n\
                   p.startDetached() }} }}"
            ))
            .unwrap();
            rt.settle();
            let entry = rt
                .load_str("import QtQuick\nQtObject { property var command: ['ri-no-such-app', '--new']; property string workingDirectory: '' }")
                .unwrap();
            crate::desktop::execute(&mut rt.world.borrow_mut(), entry);
            assert!(!crate::openurl::xdg_open("https://example.org/"));
            workpool::wait_idle();
        });
        for want in [
            "Rustshell.execDetached [\"ri-no-such-program\", \"arg\"]: failed to start: program not found",
            &format!(
                "Rustshell.execDetached [{:?}]: failed to start: ",
                plain.to_str().unwrap()
            ),
            "Rustshell.execDetached [\"/bin/sh\", \"-c\", \"true\"]: failed to start: ",
            "DesktopEntry.execute [\"ri-no-such-app\", \"--new\"]: failed to start: program not found",
            "Qt.openUrlExternally [\"xdg-open\", \"https://example.org/\"]: failed to start: program not found",
            "Process.startDetached [\"ri-no-such-process-program\", \"x\"]: failed to start: program not found",
        ] {
            assert!(text.contains(want), "no {want:?} in stderr: {text:?}");
        }
    }

    /// The signals [`super::send_signal`] sent to `pid` or its group on this thread.
    fn signals_to(pid: i32) -> Vec<(i32, i32)> {
        super::SIGNALS_SENT
            .with_borrow(|s| s.iter().copied().filter(|(p, _)| p.abs() == pid).collect())
    }

    /// Blocks until child `pid` has exited, leaving it for its Process to reap.
    fn wait_exited(pid: i32) {
        // SAFETY: an all-zero siginfo_t is a valid value for waitid(2) to fill.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        // SAFETY: `info` is writable for the call; WNOWAIT leaves the child waitable.
        while unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOWAIT,
            )
        } < 0
        {
            assert_eq!(
                std::io::Error::last_os_error().kind(),
                std::io::ErrorKind::Interrupted
            );
        }
    }

    /// Once the child is reaped its pid may belong to any new process: until the Process's exit
    /// event is delivered, `Process.signal()` ([`Proc::kill`]) and the Process's destruction
    /// ([`Proc::kill_group`]) send it nothing. Both used to signal the stale pid (and its
    /// group).
    #[test]
    fn a_reaped_child_gets_no_signal() {
        let (_dir, f) = report_fifo();
        let mut rt = Runtime::new(None);
        let r = rt
            .load_str(&format!(
                "import QtQuick\nimport Rustshell.Io\nItem {{ id: r; property int exits: 0\n\
                 Process {{ running: true; command: ['sh', '-c', 'echo $$ > \"$0\"', {f:?}]\n\
                   onExited: r.exits++ }} }}"
            ))
            .unwrap();
        rt.settle();
        let pid: i32 = std::fs::read_to_string(&f).unwrap().trim().parse().unwrap();
        wait_exited(pid);
        {
            let mut w = rt.world.borrow_mut();
            let pr = w.procs.values_mut().next().unwrap();
            assert_eq!(pr.pid, pid);
            // What the loop's next pass does first: the child's status is taken and its exit
            // event queued.
            pr.io.as_mut().unwrap().reap(&mut pr.queued);
            assert!(matches!(pr.queued.back(), Some(super::ProcEv::Exit(0))));
            pr.kill(libc::SIGTERM);
            pr.kill_group();
        }
        assert_eq!(signals_to(pid), []);
        assert!(!super::children::tracked(pid));
        run_until(&mut rt, &|rt| rt.get(r, "exits").to_number() == 1.0);
    }

    /// A child that is still running does get the signal, and its group the SIGKILL of a
    /// destroyed Process.
    #[test]
    fn a_running_child_gets_the_signal() {
        let mut rt = Runtime::new(None);
        let r = rt
            .load_str(
                "import QtQuick\nimport Rustshell.Io\nItem { id: r; property bool on: true; property int pid: 0\n\
                 function term() { l.item.signal(15) }\n\
                 Loader { id: l; active: r.on; sourceComponent: Component { Process { running: true; command: ['sleep', 'infinity']\n\
                   onStarted: r.pid = processId } } } }",
            )
            .unwrap();
        rt.settle();
        let pid = rt.get(r, "pid").to_number() as i32;
        assert!(pid > 0);
        rt.call_method(r, "term", vec![]).unwrap().unwrap();
        assert_eq!(signals_to(pid), [(pid, libc::SIGTERM)]);
        rt.set(r, "on", Value::Bool(false));
        rt.settle();
        assert!(signals_to(pid).contains(&(-pid, libc::SIGKILL)));
        crate::testenv::wait_all_gone(&[pid]);
    }

    /// Nothing scheduled: no deadline at all (Qt's event loop sleeps until an event), where the
    /// loop used to wake every second; a running Timer and a seconds clock still bound it.
    #[test]
    fn idle_has_no_deadline() {
        let mut rt = Runtime::new(None);
        rt.load_str("import QtQuick\nItem { Timer { id: t; interval: 250 } }")
            .unwrap();
        rt.settle();
        assert_eq!(rt.next_deadline(0.0), f64::INFINITY);
        let mut rt = Runtime::new(None);
        rt.load_str("import QtQuick\nItem { Timer { interval: 250; running: true } }")
            .unwrap();
        rt.settle();
        rt.tick(0.0);
        assert!(rt.next_deadline(0.0) <= 250.0);
    }

    #[test]
    fn timer_fires_repeats_and_stops() {
        let mut rt = Runtime::new(None);
        let r = rt
            .load_str(
                "import QtQuick\nItem { id: r; property int n: 0; property int m: 0\n\
                 Timer { interval: 10; running: true; repeat: true; onTriggered: { r.n++; if (r.n == 3) stop() } }\n\
                 Timer { interval: 5; running: true; onTriggered: r.m++ } }",
            )
            .unwrap();
        rt.settle();
        for t in [0.0, 4.0, 11.0, 15.0, 22.0, 33.0, 44.0, 60.0] {
            rt.tick(t);
        }
        assert_eq!(rt.get(r, "n").to_number(), 3.0);
        assert_eq!(rt.get(r, "m").to_number(), 1.0);
    }
}

/// The request target QQmlXMLHttpRequest sends for `url`: QUrl's TolerantMode parse, then
/// `toEncoded()`. The fragment is dropped; spaces, controls, non-ASCII and `"<>\\^`{|}` are
/// percent-encoded; escapes of unreserved characters are decoded and the rest upper-cased;
/// a `%` not starting a valid escape turns every `%` into `%25`.
pub fn qurl_encoded(url: &str) -> String {
    let url = url.split('#').next().unwrap_or_default();
    let start = url
        .find("://")
        .map(|i| i + 3 + url[i + 3..].find(['/', '?']).unwrap_or(url.len() - i - 3))
        .unwrap_or(0);
    let (head, rest) = url.split_at(start);
    let b = rest.as_bytes();
    let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let valid = (0..b.len())
        .filter(|&i| b[i] == b'%')
        .all(|i| i + 2 < b.len() && hex(b[i + 1]).is_some() && hex(b[i + 2]).is_some());
    let mut out = String::from(head);
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'%' && valid {
            let v = hex(b[i + 1]).unwrap() << 4 | hex(b[i + 2]).unwrap();
            if v.is_ascii_alphanumeric() || b"-._~".contains(&v) {
                out.push(v as char);
            } else {
                out.push_str(&format!("%{v:02X}"));
            }
            i += 3;
            continue;
        }
        if c <= b' ' || c >= 0x7f || b"%\"<>\\^`{|}".contains(&c) {
            out.push_str(&format!("%{c:02X}"));
        } else {
            out.push(c as char);
        }
        i += 1;
    }
    out
}

fn xhr_start(w: &mut World, a: Vec<Value>) {
    let Some(obj) = a.first().cloned() else {
        return;
    };
    let Some(req) = crate::xhr::xhr_request(&obj) else {
        return;
    };
    let (tx, rx) = w.waker.handle(crate::svc::Svc::Xhr).channel();
    let stop = crate::runtime::StopOnDrop::default();
    crate::xhr::run(req.outgoing(), stop.0.clone(), move |e| tx.send(e).is_ok());
    w.xhr.push(crate::xhr::Pending {
        obj,
        rx,
        serial: req.serial(),
        _stop: stop,
    });
}

/// One directory entry of a FolderListModel (QQuickFolderListModel roles).
struct FolderEntry {
    name: String,
    path: String,
    dir: bool,
    size: u64,
    modified: f64,
    accessed: f64,
}

/// QSaveFile: write a temp file next to the (symlink-resolved) target, keep the target's
/// permissions, then rename over it; a failed write leaves the target untouched.
pub(crate) fn save_file(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let dir = match target.parent() {
        Some(d) if !d.as_os_str().is_empty() => d,
        _ => std::path::Path::new("."),
    };
    let mut prefix = target.file_name().unwrap_or_default().to_os_string();
    prefix.push(".");
    let mut f = tempfile::Builder::new()
        .prefix(&prefix)
        .permissions(std::fs::Permissions::from_mode(0o666))
        .tempfile_in(dir)?;
    f.write_all(text.as_bytes())?;
    if let Ok(m) = std::fs::metadata(&target) {
        f.as_file().set_permissions(m.permissions())?;
    }
    f.persist(&target).map_err(|e| e.error)?;
    Ok(())
}

/// `QUrl::toLocalFile` for a `file:` URL (query and fragment dropped, a host kept as
/// `//host`, %XX decoded); a plain path as a QML `url` property resolves it. A url with
/// another scheme names no file and is answered as written.
pub fn url_to_path(u: &str) -> String {
    let url = crate::qurl::QUrl::parse(u);
    if url.is_local_file() || url.is_relative() {
        url.to_local_file()
    } else {
        u.to_string()
    }
}

/// `QUrl::fromLocalFile(path).toString()`.
pub fn path_to_url(path: &Path) -> String {
    crate::qurl::QUrl::from_local_file(path).to_string()
}

/// QRegularExpression::wildcardToRegularExpression semantics for `*`, `?` and `[...]`.
fn wildcard(p: &[char], s: &[char]) -> bool {
    match p.first() {
        None => s.is_empty(),
        Some('*') => (0..=s.len()).any(|k| wildcard(&p[1..], &s[k..])),
        Some('?') => !s.is_empty() && wildcard(&p[1..], &s[1..]),
        Some('[') if p.contains(&']') => {
            let end = p.iter().position(|c| *c == ']').unwrap();
            let Some(&c) = s.first() else { return false };
            let set = &p[1..end];
            let (neg, set) = match set.first() {
                Some('!') | Some('^') => (true, &set[1..]),
                _ => (false, set),
            };
            let mut hit = false;
            let mut k = 0;
            while k < set.len() {
                if k + 2 < set.len() && set[k + 1] == '-' {
                    hit |= set[k] <= c && c <= set[k + 2];
                    k += 3;
                } else {
                    hit |= set[k] == c;
                    k += 1;
                }
            }
            hit != neg && wildcard(&p[end + 1..], &s[1..])
        }
        Some(c) => s.first() == Some(c) && wildcard(&p[1..], &s[1..]),
    }
}

fn folder_list(w: &mut World, m: usize) -> (String, String, Vec<FolderEntry>) {
    let flag = |w: &mut World, n: &str, def: bool| {
        if w.items[m].props.contains_key(n) {
            w.read(m, n).is_some_and(|v| v.truthy())
        } else {
            def
        }
    };
    let folder = w
        .read(m, "folder")
        .map(|v| v.to_js_string())
        .unwrap_or_default();
    let filters: Vec<String> = match w.read(m, "nameFilters") {
        Some(Value::Obj(o)) => o
            .borrow()
            .array
            .iter()
            .flatten()
            .map(|v| v.to_js_string())
            .collect(),
        Some(Value::Str(s)) => vec![s.to_string()],
        _ => vec![],
    };
    let sort = if w.items[m].props.contains_key("sortField") {
        w.read(m, "sortField").map_or(1.0, |v| v.to_number()) as i64
    } else {
        1
    };
    let show_files = flag(w, "showFiles", true);
    let show_dirs = flag(w, "showDirs", true);
    let dirs_first = flag(w, "showDirsFirst", false);
    let dot = flag(w, "showDotAndDotDot", false);
    let hidden = flag(w, "showHidden", false);
    let readable = flag(w, "showOnlyReadable", false);
    let cs = flag(w, "caseSensitive", true);
    let sort_cs = flag(w, "sortCaseSensitive", true);
    let rev = flag(w, "sortReversed", false);
    let dir = url_to_path(&folder);
    let sig = format!(
        "{folder}\0{filters:?}\0{sort}{show_files}{show_dirs}{dirs_first}{dot}{hidden}{readable}{cs}{sort_cs}{rev}"
    );
    let ms = |t: std::io::Result<std::time::SystemTime>| {
        t.ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0.0, |d| d.as_millis() as f64)
    };
    let fold = |s: &str| if cs { s.to_string() } else { s.to_lowercase() };
    let pats: Vec<Vec<char>> = filters.iter().map(|f| fold(f).chars().collect()).collect();
    let mut out = vec![];
    // FileInfoThread lists QDir(""), the current directory, for an empty or missing folder.
    let dir = if dir.is_empty() || !std::path::Path::new(&dir).is_dir() {
        match std::env::current_dir() {
            Ok(c) => c.to_string_lossy().into_owned(),
            Err(_) => return (sig, dir, out),
        }
    } else {
        dir
    };
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    if dot {
        names.push(".".into());
        names.push("..".into());
    }
    for name in names {
        let path = if dir.ends_with('/') {
            format!("{dir}{name}")
        } else {
            format!("{dir}/{name}")
        };
        let Ok(md) = std::fs::metadata(&path) else {
            continue;
        };
        let is_dot = name == "." || name == "..";
        if name.starts_with('.') && !hidden && !is_dot {
            continue;
        }
        let is_dir = md.is_dir();
        if (is_dir && !show_dirs) || (!is_dir && !show_files) {
            continue;
        }
        if readable {
            let c = std::ffi::CString::new(path.as_str()).unwrap_or_default();
            // SAFETY: `c` is a live NUL-terminated path.
            if unsafe { libc::access(c.as_ptr(), libc::R_OK) } != 0 {
                continue;
            }
        }
        // QDir::AllDirs: name filters only apply to files.
        if !is_dir && !pats.is_empty() {
            let n: Vec<char> = fold(&name).chars().collect();
            if !pats.iter().any(|p| wildcard(p, &n)) {
                continue;
            }
        }
        out.push(FolderEntry {
            name,
            path,
            dir: is_dir,
            size: md.len(),
            modified: ms(md.modified()),
            accessed: ms(md.accessed()),
        });
    }
    // QDirSortItemComparator: primary key, then file name; DirsFirst and Reversed on top.
    let key = |e: &FolderEntry| {
        if sort_cs {
            e.name.clone()
        } else {
            e.name.to_lowercase()
        }
    };
    out.sort_by(|a, b| {
        use std::cmp::Ordering;
        if dirs_first && a.dir != b.dir {
            return if a.dir {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        }
        let r = match sort {
            2 => b.modified.total_cmp(&a.modified),
            3 => b.size.cmp(&a.size),
            4 => {
                let ext = |e: &FolderEntry| match e.name.rsplit_once('.') {
                    Some((_, x)) if !e.dir => x.to_lowercase(),
                    _ => String::new(),
                };
                ext(a).cmp(&ext(b))
            }
            _ => Ordering::Equal,
        };
        let r = if sort == 0 {
            r
        } else {
            r.then_with(|| key(a).cmp(&key(b)))
        };
        if rev { r.reverse() } else { r }
    });
    (sig, dir, out)
}

thread_local! {
    /// DayOfWeekRow item -> first day of week and locale its `source` was built for.
    static WEEKDAY_ROWS: std::cell::RefCell<
        std::collections::HashMap<usize, (usize, crate::locale::Locale)>,
    > =
        Default::default();
    /// DayOfWeekRow item -> the (Row, Repeater) contentItem built for it.
    static WEEKDAY_CONTENT: std::cell::RefCell<std::collections::HashMap<usize, (usize, usize)>> =
        Default::default();
}

impl Runtime {
    /// QtQuick.Controls DayOfWeekRow (Basic style): `contentItem` is a Row repeating `delegate`
    /// over `source`, the QQuickDayOfWeekModel rows (day/longName/shortName/narrowName) from
    /// `locale.firstDayOfWeek`.
    fn poll_day_of_week_rows(&mut self) -> bool {
        let rows = live_of(&mut self.world.borrow_mut(), "DayOfWeekRow");
        WEEKDAY_ROWS.with(|r| r.borrow_mut().retain(|k, _| rows.contains(k)));
        WEEKDAY_CONTENT.with(|r| r.borrow_mut().retain(|k, _| rows.contains(k)));
        let mut changed = false;
        for m in rows {
            let (name, loc, ci) = {
                let mut w = self.world.borrow_mut();
                let l = w.read(m, "locale");
                let name = match &l {
                    Some(Value::Obj(o)) => o.borrow().props.get("name").map(|v| v.to_js_string()),
                    _ => None,
                };
                let loc = l.as_ref().and_then(crate::builtins::locale_of);
                (name, loc, w.read(m, "contentItem"))
            };
            let mut fresh = false;
            if !matches!(ci, Some(Value::Item(_))) {
                // No bindings in this source: a release build has no interpreter, and runtime
                // QML text has no compiled code. The style's bindings are mirrored below.
                let src = Value::str("import QtQuick\nRow { Repeater {} }");
                if let Ok(Value::Item(c)) = crate::runtime::create_qml_object(
                    &mut self.it,
                    &Value::Undefined,
                    &[src, Value::Item(m)],
                ) {
                    let mut w = self.world.borrow_mut();
                    w.write(m, "contentItem", Value::Item(c));
                    let rep = w.items[c]
                        .children
                        .iter()
                        .copied()
                        .find(|&k| w.items[k].types.iter().any(|t| t == "Repeater"));
                    if let Some(rep) = rep {
                        WEEKDAY_CONTENT.with(|r| r.borrow_mut().insert(m, (c, rep)));
                    }
                    changed = true;
                    fresh = true;
                }
            }
            let first = match name {
                Some(n) => crate::locale::Locale::named(&n).first_day_of_week(),
                None => crate::locale::Locale::SYSTEM.first_day_of_week(),
            };
            let loc = loc.unwrap_or_else(crate::locale::Locale::system_time);
            if WEEKDAY_ROWS.with(|r| r.borrow().get(&m) != Some(&(first, loc))) {
                WEEKDAY_ROWS.with(|r| r.borrow_mut().insert(m, (first, loc)));
                let days = (0..7)
                    .map(|k| {
                        use crate::locale::Fmt;
                        let d = (first + k) % 7;
                        // QQuickDayOfWeekModel uses the standalone names.
                        let n = |f| Value::str(loc.day_name(d, f, true));
                        Runtime::js_object(&[
                            ("day", Value::Num(if d == 0 { 7.0 } else { d as f64 })),
                            ("longName", n(Fmt::Long)),
                            ("shortName", n(Fmt::Short)),
                            ("narrowName", n(Fmt::Narrow)),
                        ])
                    })
                    .collect();
                self.world
                    .borrow_mut()
                    .write(m, "source", Value::array(days));
                changed = true;
                fresh = true;
            }
            // Basic DayOfWeekRow.qml: `Row { spacing: control.spacing; Repeater { model:
            // control.source; delegate: control.delegate } }`.
            if let Some((row, rep)) = WEEKDAY_CONTENT.with(|r| r.borrow().get(&m).copied()) {
                let mut w = self.world.borrow_mut();
                if !w.dead.contains(&row) && !w.dead.contains(&rep) {
                    for (dst, to, from) in [
                        (row, "spacing", "spacing"),
                        (rep, "delegate", "delegate"),
                        (rep, "model", "source"),
                    ] {
                        // An array value never compares equal: copy `source` only when it was set.
                        if from == "source" && !fresh {
                            continue;
                        }
                        let v = w.read(m, from).unwrap_or_default();
                        changed |= w.write(dst, to, v);
                    }
                }
            }
        }
        changed
    }

    /// Qt.labs.folderlistmodel FolderListModel: lists `folder` into the `values` model rows
    /// (roles fileName/filePath/fileUrl/fileIsDir/...), re-listing on parameter or directory change.
    fn poll_folders(&mut self) -> bool {
        let models = {
            let mut w = self.world.borrow_mut();
            let mut v = live_of(&mut w, "QQuickFolderListModel");
            v.extend(live_of(&mut w, "FolderListModel"));
            v.sort_unstable();
            v.dedup();
            v
        };
        let mut folders = std::mem::take(&mut self.world.borrow_mut().folders);
        folders.retain(|k, _| models.contains(k));
        crate::fswatch::retain_folders(&mut self.world.borrow_mut(), &models);
        let mut changed = false;
        for m in models {
            let (sig, dir) = {
                let mut w = self.world.borrow_mut();
                let folder = w
                    .read(m, "folder")
                    .map(|v| v.to_js_string())
                    .unwrap_or_default();
                let filters = w
                    .read(m, "nameFilters")
                    .map(|v| v.to_js_string())
                    .unwrap_or_default();
                let mut extra = String::new();
                for n in [
                    "sortField",
                    "showFiles",
                    "showDirs",
                    "showDirsFirst",
                    "showDotAndDotDot",
                    "showHidden",
                    "showOnlyReadable",
                    "caseSensitive",
                    "sortCaseSensitive",
                    "sortReversed",
                ] {
                    if w.items[m].props.contains_key(n) {
                        extra += &w.read(m, n).map(|v| v.to_js_string()).unwrap_or_default();
                    }
                    extra.push(',');
                }
                (
                    format!("{folder}\0{filters}\0{extra}"),
                    url_to_path(&folder),
                )
            };
            // QQuickFolderListModel::componentComplete replaces a missing or unset folder with the current
            // working directory; later a missing folder is kept and lists nothing. folderChanged
            // only comes with a directoryChanged scan, which a listing filter set before the
            // first scan turns into directoryUpdated: then readers keep the requested folder.
            let first = !folders.contains_key(&m);
            if first
                && (dir.is_empty() || !std::path::Path::new(&dir).is_dir())
                && let Some(cwd) = std::env::current_dir()
                    .ok()
                    .map(|d| path_to_url(&d))
                    .filter(|c| url_to_path(c) != dir)
            {
                let mut w = self.world.borrow_mut();
                let set = |w: &mut World, n: &str, def: bool| {
                    w.items[m].props.contains_key(n)
                        && w.read(m, n).is_some_and(|v| v.truthy()) != def
                };
                let filtered = w.read(m, "nameFilters").is_some_and(|v| {
                    !matches!(v, Value::Undefined | Value::Null) && !v.to_js_string().is_empty()
                }) || [
                    ("caseSensitive", true),
                    ("showFiles", true),
                    ("showDirs", true),
                    ("showDirsFirst", false),
                    ("showDotAndDotDot", false),
                    ("showHidden", false),
                    ("showOnlyReadable", false),
                ]
                .iter()
                .any(|&(n, d)| set(&mut w, n, d));
                match w.items[m].props.get_mut("folder") {
                    Some(p) if filtered => p.value = Value::str(&cwd),
                    _ => {
                        w.write(m, "folder", Value::str(&cwd));
                    }
                }
                changed = true;
                continue;
            }
            let touched = crate::fswatch::folder_changed(&mut self.world.borrow_mut(), m, &dir);
            if folders.get(&m) == Some(&sig) && !touched {
                continue;
            }
            folders.insert(m, sig);
            let (_, dir, entries) = folder_list(&mut self.world.borrow_mut(), m);
            let mut rows = Vec::with_capacity(entries.len());
            for e in &entries {
                let modified = crate::builtins::new_date(&mut self.it, e.modified);
                let accessed = crate::builtins::new_date(&mut self.it, e.accessed);
                let (base, suffix) = match e.name.split_once('.') {
                    Some((b, _)) if !e.dir && !b.is_empty() => (
                        b.to_string(),
                        e.name.rsplit_once('.').map_or("", |x| x.1).to_string(),
                    ),
                    _ => (e.name.clone(), String::new()),
                };
                let url = Value::str(&path_to_url(Path::new(&e.path)));
                rows.push(Runtime::js_object(&[
                    ("fileName", Value::str(&e.name)),
                    ("filePath", Value::str(&e.path)),
                    // Qt 6.12 has no `fileURL` role.
                    ("fileUrl", url),
                    ("fileBaseName", Value::str(&base)),
                    ("fileSuffix", Value::str(&suffix)),
                    ("fileSize", Value::Num(e.size as f64)),
                    ("fileModified", modified),
                    ("fileAccessed", accessed),
                    ("fileIsDir", Value::Bool(e.dir)),
                ]));
            }
            let parent = std::path::Path::new(dir.trim_end_matches('/'))
                .parent()
                .map(path_to_url)
                .unwrap_or_default();
            let mut w = self.world.borrow_mut();
            for (n, t) in [
                ("values", "list"),
                ("count", "int"),
                ("parentFolder", "url"),
                ("status", "int"),
            ] {
                if !w.items[m].props.contains_key(n) {
                    w.declare(m, n, t);
                }
            }
            let n = rows.len();
            w.write(m, "values", Value::array(rows));
            w.write(m, "parentFolder", Value::str(&parent));
            w.write(m, "count", Value::Num(n as f64));
            w.write(
                m,
                "status",
                Value::Num(if dir.is_empty() { 0.0 } else { 1.0 }),
            );
            changed = true;
        }
        self.world.borrow_mut().folders = folders;
        changed
    }
}
