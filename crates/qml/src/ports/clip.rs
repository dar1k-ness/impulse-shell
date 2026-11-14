//! The `ClipboardHistory` singleton (`import Impulse`; `Cliphist.qml`, `CliphistImage.qml`):
//! `cliphist` runs once per operation with the entry on stdin (no `bash`, `printf`, `echo`),
//! its output goes to the clipboard through the process's own selection source
//! ([`super::clipsrc`], no `wl-copy`/`wl-paste`), the paste key is sent through the virtual
//! keyboard (no `ydotool`). The history is listed again when cliphist's database changes
//! (inotify), and a listing asked for while one runs is never dropped.
use std::sync::atomic::{AtomicBool, Ordering};

use super::Io;
use crate::eval::{Interp, R};
use crate::native::ProcEv;
use crate::runtime::World;
use crate::value::Value;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, TryRecvError};

#[derive(Clone, Debug, PartialEq)]
pub enum Job {
    /// `<bin> decode [args]` with `stdin` fed in, its output into the clipboard.
    Copy {
        bin: String,
        args: Vec<String>,
        stdin: Option<String>,
    },
    /// Superpaste: each entry decoded, copied and pasted once the target has read the one
    /// before. `paste` is `ClipboardHistory.pasteCommand`: empty presses Ctrl+V on the
    /// virtual keyboard, anything else is run with bash instead.
    Superpaste {
        bin: String,
        entries: Vec<String>,
        paste: String,
    },
    /// `<bin> list`, one output event per line (without its newline).
    List { bin: String },
    /// `<bin> wipe`.
    Wipe { bin: String },
    /// `<bin> delete` with the entry on stdin.
    Delete { bin: String, entry: String },
    /// `[ -f <path> ] || echo <entry> | <bin> decode > <path>`.
    DecodeTo {
        bin: String,
        entry: String,
        path: String,
    },
}

/// `ClipboardHistory.binary` (a program and its arguments) as bash splits it.
pub(crate) fn cmd(bin: &str) -> Option<Command> {
    let mut w = bin.split_ascii_whitespace();
    let mut c = workpool::userenv::command(w.next()?);
    c.args(w);
    Some(c)
}

/// Runs `bin args` with `stdin` fed in, stdout to `out` (captured when `None`); returns the
/// exit code as bash reports it and the captured stdout.
pub fn cliphist(bin: &str, args: &[&str], stdin: &str, out: Option<Stdio>) -> (i32, Vec<u8>) {
    let Some(mut c) = cmd(bin) else {
        return (127, vec![]);
    };
    let capture = out.is_none();
    c.args(args)
        .stdin(Stdio::piped())
        .stdout(out.unwrap_or_else(Stdio::piped))
        .stderr(Stdio::null());
    let Ok(mut ch) = crate::child::spawn(&mut c) else {
        return (127, vec![]);
    };
    if let Some(mut i) = ch.child_mut().stdin.take() {
        let _ = i.write_all(stdin.as_bytes());
    }
    let mut buf = vec![];
    if capture && let Some(mut o) = ch.child_mut().stdout.take() {
        let _ = std::io::Read::read_to_end(&mut o, &mut buf);
    }
    let code = ch.wait().ok().and_then(|s| s.code()).unwrap_or(1);
    (code, buf)
}

/// The source holding what decode printed, once the compositor has it; `None` when it could
/// not be set (the pipeline's status is wl-copy's: it copies whatever decode printed).
fn decode_copy(bin: &str, args: &[String], stdin: &str, stop: &crate::svc::Stop) -> Option<u64> {
    let a: Vec<&str> = std::iter::once("decode")
        .chain(args.iter().map(String::as_str))
        .collect();
    let (_, data) = cliphist(bin, &a, stdin, None);
    super::wl::copy(data, None, stop).ok()
}

/// The paste key: Ctrl down, V down, V up, Ctrl up on the virtual keyboard, or `cmd` (a
/// user's own command) when one is set.
pub(crate) fn press_paste(cmd: &str, stop: &std::sync::Arc<crate::svc::Stop>) -> bool {
    if cmd.is_empty() {
        return super::vkbd::send_wait(&[(29, true), (47, true), (47, false), (29, false)]);
    }
    super::status(workpool::userenv::command("bash").args(["-c", cmd]), stop)
        .is_some_and(|s| s.success())
}

/// Superpaste's step: pastes `source` (the selection just set) and returns once a read of it
/// asked after the paste key has been answered, another selection replaced it, or the job is
/// stopped. The first paste first waits for the shell to lose the keyboard focus
/// (`wl_keyboard.leave`), so the key goes to the window the paste is for.
fn paste_and_wait(io: &Io, cmd: &str, source: u64, first: bool) -> Option<i32> {
    use super::clipsrc::wait;
    if first {
        wait(|_| io.stopped() || !KBD_FOCUS.load(Ordering::Acquire));
    }
    if io.stopped() {
        return Some(143);
    }
    let before = wait(|_| true);
    if !press_paste(cmd, &io.stop) {
        return Some(if io.stopped() { 143 } else { 1 });
    }
    wait(|s| {
        io.stopped()
            || s.source != source
            || (s.sends > before.sends && s.answered == s.sends)
            || s.taken > before.taken
    });
    io.stopped().then_some(143)
}

impl Job {
    pub fn run(self, io: &Io) -> i32 {
        match self {
            Job::Copy { bin, args, stdin } => {
                match decode_copy(&bin, &args, stdin.as_deref().unwrap_or(""), &io.stop) {
                    Some(_) => 0,
                    None if io.stopped() => 143,
                    None => 1,
                }
            }
            Job::Superpaste {
                bin,
                entries,
                paste,
            } => {
                // Stopped (a newer superpaste, the World going away): no further paste lands
                // in whatever window has focus by then.
                let _notify = io.stop.on_stop(super::clipsrc::notify);
                for (k, e) in entries.iter().enumerate() {
                    if io.stopped() {
                        return 143;
                    }
                    let Some(source) = decode_copy(&bin, &[], e, &io.stop) else {
                        return if io.stopped() { 143 } else { 1 };
                    };
                    if let Some(code) = paste_and_wait(io, &paste, source, k == 0) {
                        return code;
                    }
                }
                0
            }
            Job::List { bin } => {
                let (code, out) = cliphist(&bin, &["list"], "", None);
                for line in String::from_utf8_lossy(&out).lines() {
                    io.out(line);
                }
                code
            }
            Job::Wipe { bin } => cliphist(&bin, &["wipe"], "", Some(Stdio::null())).0,
            Job::Delete { bin, entry } => {
                cliphist(
                    &bin,
                    &["delete"],
                    &format!("{entry}\n"),
                    Some(Stdio::null()),
                )
                .0
            }
            // The original's `/tmp/rustshell/media/cliphist` is shared: the directory must be ours
            // and the file is decoded to a temporary in it, renamed into place when done (a
            // symlink planted there is replaced, never written through; a failed decode
            // leaves no truncated image behind for the `-f` check).
            Job::DecodeTo { bin, entry, path } => {
                let Ok((dir, name)) = crate::privfs::Dir::parent_of(path.as_ref(), false) else {
                    return 1;
                };
                if dir.has_file(name) {
                    return 0;
                }
                let Ok((f, pending)) = dir.create(name) else {
                    return 1;
                };
                let code = cliphist(
                    &bin,
                    &["decode"],
                    &format!("{entry}\n"),
                    Some(Stdio::from(f)),
                )
                .0;
                if code == 0 && pending.commit().is_err() {
                    return 1;
                }
                code
            }
        }
    }
}

/// The running superpaste: starting another stops it, so two never interleave their pastes.
static SUPERPASTE: std::sync::Mutex<Option<Running>> = std::sync::Mutex::new(None);

/// A superpaste's stop and the World's wakeup that stops it with the World, unregistered
/// when the next superpaste replaces it.
struct Running {
    stop: std::sync::Arc<crate::svc::Stop>,
    _world: Option<crate::svc::OnStop<'static>>,
}

/// The name of the watch on cliphist's database ([`crate::fswatch::path_changed`]).
const WATCH: &str = "cliphist";

fn is_classic(bin: &str) -> bool {
    bin.contains("cliphist")
}

/// Stash: `entry.split("\t")[0]`.
fn entry_number(entry: &str) -> String {
    entry.split('\t').next().unwrap_or_default().to_owned()
}

/// cliphist's database: `$CLIPHIST_DB_PATH`, else `db-path` from its config file
/// (`$CLIPHIST_CONFIG_PATH` or `$XDG_CONFIG_HOME/cliphist/config`), else
/// `$XDG_CACHE_HOME/cliphist/db`.
pub fn cliphist_db() -> PathBuf {
    let env = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if let Some(p) = env("CLIPHIST_DB_PATH") {
        return p;
    }
    let home = env("HOME").unwrap_or_default();
    let config = env("CLIPHIST_CONFIG_PATH").unwrap_or_else(|| {
        env("XDG_CONFIG_HOME")
            .unwrap_or_else(|| home.join(".config"))
            .join("cliphist/config")
    });
    if let Ok(s) = std::fs::read_to_string(config) {
        let db = s.lines().find_map(|l| {
            let (k, v) = l.trim().split_once(char::is_whitespace)?;
            (k == "db-path").then(|| v.trim().to_string())
        });
        if let Some(db) = db.filter(|d| !d.is_empty()) {
            return PathBuf::from(db);
        }
    }
    env("XDG_CACHE_HOME")
        .unwrap_or_else(|| home.join(".cache"))
        .join("cliphist/db")
}

/// A job of the history started for QML and what its exit means.
struct Run {
    rx: Receiver<ProcEv>,
    then: Then,
    /// Stops the job with the World.
    _world: crate::svc::OnStop<'static>,
}

enum Then {
    /// `list`: the lines printed so far become `entries` on exit 0.
    List(Vec<String>),
    /// `delete`, `wipe`: the history changed, list it again.
    Refresh,
    /// `decodeImage(entry, path)`: `imageDecoded(path, ok)`.
    Decoded(String),
}

/// State of the `ClipboardHistory` singleton (`import Impulse`): the jobs it runs, the
/// database it watches and whether a listing is owed.
pub struct History {
    item: usize,
    db: PathBuf,
    /// `refresh()` was called: the database is watched and its changes list again.
    watch: bool,
    /// The history changed while a listing ran (that listing may predate the change): list
    /// again once it has been delivered.
    dirty: bool,
    runs: Vec<Run>,
}

impl History {
    fn listing(&self) -> bool {
        self.runs.iter().any(|r| matches!(r.then, Then::List(_)))
    }

    fn start(&mut self, w: &mut World, job: Job, then: Then) {
        let (tx, rx) = w.waker.handle(crate::svc::Svc::Clipboard).channel();
        let stop = super::Job::Clip(job).start(Some(tx));
        let world = w
            .jobs_stop
            .0
            .on_stop_owned(move || stop.store(true, Ordering::Release));
        self.runs.push(Run {
            rx,
            then,
            _world: world,
        });
    }

    /// Whether the history is the database being watched: cliphist's. Another tool keeps
    /// its history elsewhere, so nothing is watched for it and `Cliphist.qml` lists again
    /// itself after a copy.
    fn follows_db(&self, w: &mut World) -> bool {
        self.watch && is_classic(&binary(w, self.item))
    }

    fn list(&mut self, w: &mut World) {
        self.dirty = false;
        let bin = binary(w, self.item);
        self.start(w, Job::List { bin }, Then::List(Vec::new()));
    }
}

/// `ClipboardHistory.binary`: the history tool (`cliphist`, or a stash-like one).
fn binary(w: &mut World, item: usize) -> String {
    match w.read(item, "binary") {
        Some(v) if !v.to_js_string().is_empty() => v.to_js_string(),
        _ => "cliphist".into(),
    }
}

/// Runs `f` with the World and the state of the singleton `this`.
fn with<T>(
    it: &mut Interp,
    this: &Value,
    f: impl FnOnce(&mut World, &mut History) -> T,
) -> Option<T> {
    let Value::Item(item) = *this else {
        return None;
    };
    let w = crate::runtime::world_of(it)?;
    let w = &mut *w.borrow_mut();
    let mut h = w.cliphist.take().unwrap_or_else(|| History {
        item,
        db: cliphist_db(),
        watch: false,
        dirty: false,
        runs: Vec::new(),
    });
    h.item = item;
    let r = f(w, &mut h);
    w.cliphist = Some(h);
    Some(r)
}

fn text(args: &[Value], i: usize) -> String {
    args.get(i).map(Value::to_js_string).unwrap_or_default()
}

/// [`crate::impulse::hooked`] for `method` of the singleton: under a spawn hook no history
/// tool runs, nothing is copied or pasted and no file is written or removed.
fn hooked(method: &str, args: &[&str]) -> bool {
    crate::impulse::hooked("ClipboardHistory", method, args)
}

/// `ClipboardHistory` (qmltypes/Impulse/clipboard-history.qmltypes).
pub static TYPE: crate::impulse::Type = crate::impulse::Type {
    name: "ClipboardHistory",
    methods: &[
        ("refresh", refresh),
        ("watched", watched),
        ("copy", copy),
        ("superpaste", superpaste),
        ("remove", remove),
        ("wipe", wipe),
        ("decodeImage", decode_image),
        ("removeImage", remove_image),
    ],
    init: Some(|w, id| {
        w.write(id, "binary", Value::str("cliphist"));
    }),
};

/// `refresh()`: lists the history into `entries`. A call while a listing runs is owed and
/// runs once that listing has been delivered (the history may have changed after it was
/// read). From the first call on the database is watched and every change lists again.
fn refresh(it: &mut Interp, this: &Value, _: &[Value]) -> R<Value> {
    if hooked("refresh", &[]) {
        return Ok(Value::Undefined);
    }
    with(it, this, |w, h| {
        h.watch = true;
        if h.listing() {
            h.dirty = true;
        } else {
            h.list(w);
        }
    });
    Ok(Value::Undefined)
}

/// `watched()`: whether the history's database is watched, so a copy lists again by itself.
/// False for a tool other than cliphist.
fn watched(it: &mut Interp, this: &Value, _: &[Value]) -> R<Value> {
    let on = with(it, this, |w, h| {
        h.follows_db(w) && crate::fswatch::path_watched(w, WATCH, &h.db)
    });
    Ok(Value::Bool(on == Some(true)))
}

/// `copy(entry)`: the entry decoded into the clipboard. cliphist takes the entry on stdin,
/// a stash-like tool its number as an argument.
fn copy(it: &mut Interp, this: &Value, args: &[Value]) -> R<Value> {
    let entry = text(args, 0);
    if hooked("copy", &[&entry]) {
        return Ok(Value::Undefined);
    }
    with(it, this, |w, h| {
        let bin = binary(w, h.item);
        let job = if is_classic(&bin) {
            Job::Copy {
                bin,
                args: vec![],
                stdin: Some(entry),
            }
        } else {
            Job::Copy {
                bin,
                args: vec![entry_number(&entry)],
                stdin: None,
            }
        };
        super::Job::Clip(job).start(None);
    });
    Ok(Value::Undefined)
}

/// `superpaste(entries)`: each entry copied and pasted in turn, the next once the window
/// has read the one before. Starting another stops the one running.
fn superpaste(it: &mut Interp, this: &Value, args: &[Value]) -> R<Value> {
    let entries: Vec<String> = match args.first() {
        Some(Value::Obj(o)) => o
            .borrow()
            .array
            .iter()
            .flatten()
            .map(Value::to_js_string)
            .collect(),
        _ => vec![],
    };
    let shown: Vec<&str> = entries.iter().map(String::as_str).collect();
    if entries.is_empty() || hooked("superpaste", &shown) {
        return Ok(Value::Undefined);
    }
    with(it, this, |w, h| {
        let bin = binary(w, h.item);
        let paste = w
            .read(h.item, "pasteCommand")
            .map(|v| v.to_js_string())
            .unwrap_or_default();
        let stop = super::Job::Clip(Job::Superpaste {
            bin,
            entries,
            paste,
        })
        .start(None);
        let s = stop.clone();
        let world = w
            .jobs_stop
            .0
            .on_stop_owned(move || s.store(true, Ordering::Release));
        let mut cur = SUPERPASTE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(old) = cur.replace(Running {
            stop,
            _world: Some(world),
        }) {
            old.stop.store(true, Ordering::Release);
        }
    });
    Ok(Value::Undefined)
}

/// `remove(entry)`: the entry deleted from the history, which is then listed again.
fn remove(it: &mut Interp, this: &Value, args: &[Value]) -> R<Value> {
    let entry = text(args, 0);
    if hooked("remove", &[&entry]) {
        return Ok(Value::Undefined);
    }
    with(it, this, |w, h| {
        let bin = binary(w, h.item);
        h.start(w, Job::Delete { bin, entry }, Then::Refresh);
    });
    Ok(Value::Undefined)
}

/// `wipe()`: the whole history deleted, then listed again.
fn wipe(it: &mut Interp, this: &Value, _: &[Value]) -> R<Value> {
    if hooked("wipe", &[]) {
        return Ok(Value::Undefined);
    }
    with(it, this, |w, h| {
        let bin = binary(w, h.item);
        h.start(w, Job::Wipe { bin }, Then::Refresh);
    });
    Ok(Value::Undefined)
}

/// `decodeImage(entry, path)`: the entry's image written to `path` unless the file is
/// there already; `imageDecoded(path, ok)` when done.
fn decode_image(it: &mut Interp, this: &Value, args: &[Value]) -> R<Value> {
    let (entry, path) = (text(args, 0), text(args, 1));
    if hooked("decodeImage", &[&entry, &path]) {
        // Nothing was decoded: the answer of a decode that failed.
        with(it, this, |w, h| {
            let item = h.item;
            crate::impulse::replies(w).send(move |w| decoded(w, item, &path, false));
        });
        return Ok(Value::Undefined);
    }
    with(it, this, |w, h| {
        let bin = binary(w, h.item);
        let job = Job::DecodeTo {
            bin,
            entry,
            path: path.clone(),
        };
        h.start(w, job, Then::Decoded(path));
    });
    Ok(Value::Undefined)
}

/// `removeImage(path)`: a decoded image file removed.
fn remove_image(_: &mut Interp, _: &Value, args: &[Value]) -> R<Value> {
    let p = text(args, 0);
    if !hooked("removeImage", &[&p]) && std::path::Path::new(&p).is_file() {
        let _ = std::fs::remove_file(&p);
    }
    Ok(Value::Undefined)
}

/// `imageDecoded(path, ok)` of the singleton `item`.
fn decoded(w: &mut World, item: usize, path: &str, ok: bool) {
    let args = vec![Value::str(path), Value::Bool(ok)];
    crate::native::emit(w, item, "imageDecoded", &["path", "ok"], args);
}

/// Whether a listing runs or has ended without its lines being in `entries` yet.
pub fn listing(w: &World) -> bool {
    w.cliphist.as_ref().is_some_and(History::listing)
}

/// Delivers what the history's jobs reported and starts the listing that is owed: after a
/// database change, a `remove`/`wipe`, or a `refresh()` asked while one ran.
pub(crate) fn poll(w: &mut World) -> bool {
    let Some(mut h) = w.cliphist.take() else {
        return false;
    };
    if w.dead.contains(&h.item) {
        return false;
    }
    let mut changed = false;
    if h.follows_db(w) {
        if crate::fswatch::path_changed(w, WATCH, &h.db) {
            h.dirty = true;
        }
    } else {
        crate::fswatch::path_released(w, WATCH);
    }
    let mut k = 0;
    while k < h.runs.len() {
        let run = &mut h.runs[k];
        let code = loop {
            match run.rx.try_recv() {
                Ok(ProcEv::Out(line)) => {
                    if let Then::List(lines) = &mut run.then {
                        lines.push(line);
                    }
                }
                Ok(ProcEv::Exit(c)) => break Some(c),
                Ok(ProcEv::Signaled(_)) | Err(TryRecvError::Disconnected) => break Some(-1),
                Ok(ProcEv::Err(_) | ProcEv::File { .. }) => {}
                Err(TryRecvError::Empty) => break None,
            }
        };
        let Some(code) = code else {
            k += 1;
            continue;
        };
        changed = true;
        match h.runs.remove(k).then {
            Then::List(lines) if code == 0 => {
                let v = Value::array(lines.iter().map(|l| Value::str(l)).collect());
                if !w.write(h.item, "entries", v) {
                    w.notify(h.item, "entries");
                }
            }
            Then::List(_) => {
                w.errors
                    .push(format!("ClipboardHistory: listing failed with code {code}"));
            }
            Then::Refresh => h.dirty = true,
            Then::Decoded(path) => decoded(w, h.item, &path, code == 0),
        }
    }
    if h.dirty && !h.listing() {
        h.list(w);
        changed = true;
    }
    w.cliphist = Some(h);
    changed
}

/// The selection text the watcher saw last, for the main loop.
static SELECTION_TEXT: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
/// The watcher saw a selection the shell has not been told about yet.
static SELECTION_NEW: AtomicBool = AtomicBool::new(false);
/// A shell surface has keyboard focus.
static KBD_FOCUS: AtomicBool = AtomicBool::new(false);
/// Keyboard focus entered a shell surface since the last poll.
static KBD_ENTERED: AtomicBool = AtomicBool::new(false);

/// Keyboard focus entered (`true`) or left every shell surface. Qt on Wayland only gets the
/// selection through wl_data_device, which the compositor sends to the focused client: once on
/// focus enter and on every change while focused.
pub fn keyboard_focus(waker: &crate::svc::Waker, on: bool) {
    KBD_FOCUS.store(on, Ordering::Release);
    super::clipsrc::notify();
    if on {
        KBD_ENTERED.store(true, Ordering::Relaxed);
        waker.wake(crate::svc::Svc::Clipboard);
    }
}

/// `Rustshell.clipboardText = text` (QClipboard::setText).
pub fn set_text(text: String) {
    super::clipsrc::set(super::clipsrc::text_offers(text));
}

/// `QClipboard::text()` for a paste while the clipboard is this process's own
/// ([`super::clipsrc::own_text`]); `None` when a paste has to ask the compositor's offer.
pub fn own_text() -> Option<String> {
    super::clipsrc::own_text()
}

/// `Rustshell.clipboardText` follows the selection (QClipboard::dataChanged) the way Qt sees it:
/// only while a shell surface has keyboard focus, re-announced on each focus enter. The watcher
/// starts with the first poll that has the Quickshell singleton.
pub fn poll_text(w: &mut crate::runtime::World) -> bool {
    static WATCH: std::sync::Once = std::sync::Once::new();
    let Some(&qs) = w.singletons.get("Rustshell") else {
        return false;
    };
    WATCH.call_once(|| {
        // Unit tests feed SELECTION_TEXT themselves; a live compositor's selection would race them.
        if std::env::var_os("WAYLAND_DISPLAY").is_some() && !cfg!(test) {
            let h = w.waker.handle(crate::svc::Svc::Clipboard);
            super::wl::watch_text(move |t| {
                *SELECTION_TEXT.lock().unwrap() = Some(t);
                SELECTION_NEW.store(true, Ordering::Relaxed);
                h.wake();
            });
        }
    });
    if !KBD_FOCUS.load(Ordering::Relaxed) {
        return false;
    }
    let entered = KBD_ENTERED.swap(false, Ordering::Relaxed);
    let fresh = SELECTION_NEW.swap(false, Ordering::Relaxed);
    if !entered && !fresh {
        return false;
    }
    let Some(t) = SELECTION_TEXT.lock().unwrap().clone() else {
        return false;
    };
    if !w.write(qs, "clipboardText", Value::str(&t)) {
        w.notify(qs, "clipboardText");
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::Runtime;

    fn select(t: &str) {
        *SELECTION_TEXT.lock().unwrap() = Some(t.into());
        SELECTION_NEW.store(true, Ordering::Relaxed);
    }

    #[test]
    fn superpaste_honours_stop() {
        let dir = std::env::temp_dir().join(format!("ri-superpaste-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("calls");
        let bin = dir.join("cliphist");
        crate::testenv::install_script(
            &bin,
            &format!("#!/bin/bash\necho \"$@\" >> '{}'\n", log.display()),
        );
        let job = || Job::Superpaste {
            bin: bin.display().to_string(),
            entries: vec!["1\ta".into(), "2\tb".into()],
            paste: "true".into(),
        };
        // Stopped before it ran: nothing is decoded, copied or pasted.
        let io = Io::detached();
        io.stop.store(true, Ordering::Release);
        assert_eq!(job().run(&io), 143);
        assert!(!log.exists(), "cliphist ran after stop");
        // A paste nobody reads waits for the read, not a timer: the stop ends it.
        let io = Io::detached();
        let _notify = io.stop.on_stop_owned(crate::ports::clipsrc::notify);
        let s = io.stop.clone();
        let source = crate::ports::clipsrc::wait(|_| true).source;
        let waiting = std::thread::spawn(move || paste_and_wait(&io, "true", source, false));
        s.store(true, Ordering::Release);
        assert_eq!(waiting.join().unwrap(), Some(143));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The watched database is cliphist's. With another history tool as `binary` it is not
    /// that tool's history: `watched()` says no (Cliphist.qml then lists again itself after a
    /// copy), and a change of cliphist's database lists nothing.
    #[test]
    fn only_cliphist_is_watched_through_its_database() {
        let dir = std::env::temp_dir().join(format!("ri-clip-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        let log = dir.join("calls");
        for tool in ["cliphist", "stash"] {
            crate::testenv::install_script(
                &dir.join("bin").join(tool),
                &format!("#!/bin/sh\necho \"{tool} $*\" >> '{}'\n", log.display()),
            );
        }
        let db = dir.join("db");
        std::fs::write(&db, "0").unwrap();
        let mut rt = Runtime::new(None);
        let r = rt
            .load_str(
                "import QtQuick\nimport Impulse\nItem {\n\
                 function list(bin) { ClipboardHistory.binary = bin; ClipboardHistory.refresh() }\n\
                 function watched() { return ClipboardHistory.watched() } }",
            )
            .unwrap();
        rt.settle();
        let calls = || std::fs::read_to_string(&log).unwrap_or_default();
        let idle = |rt: &Runtime| !listing(&rt.world.borrow());
        let watched = |rt: &mut Runtime| match rt.call_method(r, "watched", vec![]) {
            Some(Ok(v)) => v.truthy(),
            e => panic!("{e:?}"),
        };
        let list = |rt: &mut Runtime, tool: &str| {
            let bin = dir.join("bin").join(tool).display().to_string();
            rt.call_method(r, "list", vec![Value::str(&bin)]);
            // The singleton's state exists from its first call: its database is the test's.
            rt.world.borrow_mut().cliphist.as_mut().unwrap().db = db.clone();
            crate::native::tests::run_until(rt, &idle);
        };
        list(&mut rt, "cliphist");
        assert!(watched(&mut rt), "cliphist's database is watched");
        std::fs::write(&db, "1").unwrap();
        rt.poll_native();
        assert!(!idle(&rt), "a change of cliphist's database lists again");
        crate::native::tests::run_until(&mut rt, &idle);
        assert_eq!(calls(), "cliphist list\ncliphist list\n");
        list(&mut rt, "stash");
        assert_eq!(calls(), "cliphist list\ncliphist list\nstash list\n");
        assert!(
            !watched(&mut rt),
            "stash's history is not cliphist's database"
        );
        std::fs::write(&db, "2").unwrap();
        rt.poll_native();
        assert!(idle(&rt), "cliphist's database is not stash's history");
        // Back to cliphist: watched again.
        list(&mut rt, "cliphist");
        assert!(watched(&mut rt));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The user's own paste command (`bash -c`) is a child of the job: `running = false`
    /// ends it and the job.
    #[test]
    fn stopping_superpaste_ends_its_paste_command() {
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("pid");
        crate::testenv::fifo(&fifo);
        let cmd = format!("echo $$ > '{}'; exec sleep infinity", fifo.display());
        crate::ports::tests::stop_ends_the_program("bash -c", &fifo, move |io| {
            let source = crate::ports::clipsrc::wait(|_| true).source;
            paste_and_wait(io, &cmd, source, false).unwrap_or(0)
        });
    }

    #[test]
    fn clipboard_text_follows_the_selection_only_with_keyboard_focus() {
        let mut rt = Runtime::new(None);
        let r = rt
            .load_str(
                "import QtQuick\nimport Quickshell\nItem { id: r; property int n; property string t: Rustshell.clipboardText\n\
                 Connections { target: Rustshell; function onClipboardTextChanged() { r.n++ } } }",
            )
            .unwrap();
        rt.settle();
        let poll = |rt: &mut Runtime| {
            let c = poll_text(&mut rt.world.borrow_mut());
            rt.settle();
            (c, rt.get(r, "n").to_number(), rt.get(r, "t").to_js_string())
        };
        let waker = rt.world.borrow().waker.clone();
        let focus = |on| keyboard_focus(&waker, on);
        focus(false);
        select("a");
        assert_eq!(poll(&mut rt), (false, 0.0, "".into()));
        focus(true);
        assert_eq!(poll(&mut rt), (true, 1.0, "a".into()));
        assert_eq!(poll(&mut rt), (false, 1.0, "a".into()));
        select("b");
        assert_eq!(poll(&mut rt), (true, 2.0, "b".into()));
        focus(false);
        focus(true);
        assert_eq!(poll(&mut rt), (true, 3.0, "b".into()));
        focus(false);
    }
}
