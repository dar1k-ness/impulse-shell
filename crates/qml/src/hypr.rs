//! Native `Rustshell.Hyprland` IPC: request socket for state, event socket for `rawEvent`.

use crate::runtime::{Runtime, World};
use crate::svc::Svc;
use crate::value::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, Read, Write};
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};

pub struct Hypr {
    rx: Events,
    /// Requests go to the `requests` worker, never out of the main thread.
    req: Sender<Req>,
    replies: Receiver<Reply>,
    /// A refresh is out at the worker.
    pending: bool,
    mons: BTreeMap<i64, usize>,
    /// Workspace address (0.57+; the numeric id as a string before) -> HyprlandWorkspace.
    wss: BTreeMap<String, usize>,
    /// address (no 0x) -> (wayland Toplevel, HyprlandToplevel)
    tops: BTreeMap<String, (usize, usize)>,
    /// ToplevelManager singleton the toplevel model was last published to.
    tm: Option<usize>,
    dirty: bool,
    /// A monitors reply was parsed (HyprlandIpc::monitorsRequested).
    mons_requested: bool,
    /// Addresses (no 0x) of the windows an `urgent` event named and nothing cleared since.
    urgent: BTreeSet<String>,
}

/// `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE`, Hyprland's `m_instancePath`.
fn instance_dir() -> Option<PathBuf> {
    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
    let rt = std::env::var("XDG_RUNTIME_DIR").ok()?;
    Some(Path::new(&rt).join("hypr").join(sig))
}

pub fn socket_dir() -> Option<PathBuf> {
    instance_dir().filter(|d| d.join(".socket.sock").exists())
}

/// Hyprland's instance lock (`CCompositor::createLockFile`), which `hyprctl instances` lists
/// instances by.
const LOCK: &str = "hyprland.lock";

/// Whether the instance at `dir` serves requests: Hyprland creates [`LOCK`] in
/// `startCompositor`, after `initManagers` bound and listened on both sockets, while
/// `.socket.sock` alone may be bound but not listening yet.
fn instance_up(dir: &Path) -> bool {
    dir.join(".socket.sock").exists() && dir.join(LOCK).exists()
}

/// A `Hyprland` singleton's wait for its instance ([`wait`]), kept in `World::hypr_wait`.
/// Dropping it (with its World) ends the `hypr-wait` thread and closes its inotify instance.
pub struct Wait {
    rx: Receiver<PathBuf>,
    /// The eventfd that wakes the thread to end.
    stop: Option<Arc<OwnedFd>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Wait {
    /// The instance directory, once the instance there is up.
    fn ready(&self) -> Option<PathBuf> {
        self.rx.try_recv().ok()
    }
}

impl Drop for Wait {
    fn drop(&mut self) {
        if let Some(stop) = &self.stop {
            crate::sysbus::signal(stop);
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Sends `dir` once the Hyprland instance there is up (see [`instance_up`]), for a `Hyprland`
/// singleton made before it was. Hyprland creates `$XDG_RUNTIME_DIR/hypr`, then the instance
/// directory, then its sockets and the lock, so the thread sleeps on an inotify watch of each
/// of those directories that exists, watching a directory before looking into it so nothing
/// created meanwhile is missed, and looks again on each event in them, with no time limit;
/// dropping the [`Wait`] wakes it to end. Without `$XDG_RUNTIME_DIR`, inotify or an eventfd
/// (instance or fd limit) or the thread (the system refused it) nothing is sent, as without an
/// instance.
fn wait(dir: PathBuf, h: &crate::svc::Handle) -> Wait {
    let (tx, rx) = h.channel();
    let Some(stop) = crate::sysbus::eventfd().map(Arc::new) else {
        return Wait {
            rx,
            stop: None,
            thread: None,
        };
    };
    let s = stop.clone();
    let thread = std::thread::Builder::new()
        .name("hypr-wait".into())
        .spawn(move || {
            if wait_up(&dir, &s) {
                let _ = tx.send(dir);
            }
        })
        .inspect_err(|e| elog::eprintln!("hyprland: cannot start the hypr-wait thread: {e}"))
        .ok();
    Wait {
        rx,
        stop: Some(stop),
        thread,
    }
}

fn wait_up(dir: &Path, stop: &OwnedFd) -> bool {
    use inotify::WatchMask;
    let Ok(mut ino) = inotify::Inotify::init() else {
        return false;
    };
    let hypr = dir.parent();
    let chain = [hypr.and_then(Path::parent), hypr, Some(dir)];
    let mask = WatchMask::CREATE | WatchMask::MOVED_TO | WatchMask::ONLYDIR;
    let mut buf = [0u8; 4096];
    loop {
        for (i, d) in chain.iter().enumerate() {
            // `$XDG_RUNTIME_DIR` holds the rest; a directory not created yet gets its watch
            // on the event of its parent that created it.
            if let Some(d) = d
                && ino.watches().add(d, mask).is_err()
                && i == 0
            {
                return false;
            }
        }
        if instance_up(dir) {
            return true;
        }
        if crate::sysbus::readable_or_stopped(stop, ino.as_fd())
            || ino.read_events_blocking(&mut buf).is_err()
        {
            return false;
        }
    }
}

/// Hyprland's name for a `zwp_virtual_keyboard_v1` keyboard: `hl-virtual-keyboard`, or with
/// `misc:name_vk_after_proc` (the default) `hl-virtual-keyboard-<process>`.
pub fn is_virtual_keyboard(name: &str) -> bool {
    name.strip_prefix("hl-virtual-keyboard")
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('-'))
}

pub fn request(dir: &Path, cmd: &str) -> Option<String> {
    request_at(&dir.join(".socket.sock"), cmd)
}

pub use hypripc::{IpcError, exchange};

/// Hyprland answers `.socket.sock` requests one at a time on its main loop, so one-off
/// requests (hyprctl calls, focus and exit dispatches) go out one at a time too, in the order
/// they were made. With no deadline on a reply ([`exchange`]), a busy or stuck Hyprland then
/// holds this one lane, never the bounded workers that logind and D-Bus actions queue for.
pub fn submit(f: impl FnOnce() + Send + 'static) {
    static LANE: workpool::Lane = workpool::Lane::new();
    LANE.run(f);
}

pub fn request_at(sock: &Path, cmd: &str) -> Option<String> {
    exchange(sock, cmd, None).ok()
}

/// `hyprctl hyprsunset CMD` (hyprctl's `requestIPC`): write the request, then ONE read of up
/// to 8 KiB with no timeout, and print it. hyprsunset keeps the connection open for further
/// requests and answers only once its main loop handled this one, so a client that waits for
/// EOF or gives up early closes the socket before the reply: hyprsunset's reply `write` then
/// raises SIGPIPE and kills it.
pub fn request_ipc(sock: &Path, cmd: &str) -> String {
    let Ok(mut s) = UnixStream::connect(sock) else {
        return format!("Couldn't connect to {}. (3)\n", sock.display());
    };
    if let Err(e) = s.write_all(cmd.as_bytes()) {
        // hyprctl itself dies of the SIGPIPE, printing nothing.
        return if e.kind() == std::io::ErrorKind::BrokenPipe {
            String::new()
        } else {
            "Couldn't write (4)\n".into()
        };
    }
    let mut buf = [0u8; 8192];
    let n = loop {
        match s.read(&mut buf) {
            Ok(n) => break n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return "Couldn't read (5)\n".into(),
        }
    };
    String::from_utf8_lossy(&buf[..n]).into_owned() + "\n"
}

enum Req {
    Dispatch(String, String),
    Refresh,
    Status,
}

/// What the `requests` worker sends back to the loop.
enum Reply {
    /// `None`: the snapshot failed.
    Snapshot(Option<Raw>),
    UsingLua(bool),
}

/// Raw `j/monitors`, `j/workspaces`, `j/activeworkspace`, `j/clients`, `j/activewindow`.
type Raw = [String; 5];

/// The five queries as one `hyprctl --batch` request: Hyprland's `CSocket1::dispatchBatch`
/// answers every command of a `[[BATCH]]` request in the same turn of its event loop, so the
/// replies describe one state (no workspace created or destroyed between them).
const SNAPSHOT: &str =
    "[[BATCH]]j/monitors;j/workspaces;j/activeworkspace;j/clients;j/activewindow";
/// What `joinBatchReplies` puts between the replies of a batch.
const BATCH_DELIMITER: &str = "\n\n\n";

static SYNC: AtomicBool = AtomicBool::new(false);

/// Wait for each refresh in the turn that asked for it (lockstep checkers need the same
/// state at the same step); the shell itself never waits on Hyprland.
pub fn set_sync(on: bool) {
    SYNC.store(on, Ordering::Relaxed);
}

/// Requests handed to a `hypr-req` worker and not answered yet ([`drain`]).
static QUEUED: (std::sync::Mutex<usize>, std::sync::Condvar) =
    (std::sync::Mutex::new(0), std::sync::Condvar::new());

fn queue(req: &Sender<Req>, r: Req) -> bool {
    *QUEUED.0.lock().unwrap_or_else(|e| e.into_inner()) += 1;
    let ok = req.send(r).is_ok();
    if !ok {
        answered();
    }
    ok
}

fn answered() {
    let mut n = QUEUED.0.lock().unwrap_or_else(|e| e.into_inner());
    *n -= 1;
    if *n == 0 {
        QUEUED.1.notify_all();
    }
}

/// Blocks until every dispatch and refresh queued so far has had Hyprland's answer (or failed).
/// For a host that is about to exit, and tests.
pub fn drain() {
    let mut n = QUEUED.0.lock().unwrap_or_else(|e| e.into_inner());
    while *n != 0 {
        n = QUEUED.1.wait(n).unwrap_or_else(|e| e.into_inner());
    }
}

/// Request worker: dispatches and refreshes in order, replies back to the loop. Where the
/// system refuses its thread the sender is disconnected: [`queue`] reports every request as
/// not sent.
fn requests(dir: &Path, h: &crate::svc::Handle) -> (Sender<Req>, Receiver<Reply>) {
    let (tx, rx) = std::sync::mpsc::channel::<Req>();
    let (rtx, rrx) = h.channel();
    let dir = dir.to_path_buf();
    let spawned = std::thread::Builder::new()
        .name("hypr-req".into())
        .spawn(move || {
            // Every request queued is carried out and answered (`drain`), also those behind a
            // refresh whose reply nobody reads any more (the Hypr is gone).
            let mut unread = false;
            for r in rx {
                match r {
                    Req::Dispatch(cmd, d) => {
                        let r = exchange(&dir.join(".socket.sock"), &cmd, None);
                        if let Some(e) = dispatch_error(r) {
                            elog::eprintln!("hyprland dispatch {d:?} failed{e}");
                        }
                    }
                    Req::Refresh if !unread => {
                        unread = rtx.send(Reply::Snapshot(snapshot(&dir))).is_err();
                    }
                    Req::Refresh => {}
                    Req::Status if !unread => match request(&dir, "j/status") {
                        Some(r) => unread = rtx.send(Reply::UsingLua(using_lua(&r))).is_err(),
                        None => elog::eprintln!("Hyprland ipc status request failed."),
                    },
                    Req::Status => {}
                }
                answered();
            }
        });
    if let Err(e) = spawned {
        elog::eprintln!("hyprland: cannot start the hypr-req thread: {e}");
    }
    (tx, rrx)
}

/// `Hyprland.usingLua` from the `j/status` reply, as Quickshell's HyprlandIpc reads it: the
/// reply's `configProvider` is `lua`. A Hyprland from before the request answers
/// `unknown request`, which is no JSON object.
fn using_lua(status: &str) -> bool {
    crate::json::parse(status)
        .is_ok_and(|v| matches!(field(&v, "configProvider"), Value::Str(p) if &*p == "lua"))
}

/// The window an `urgent` or `closewindow` event names, as the address the
/// toplevels are kept by: Hyprland writes it in hex with no `0x`.
fn event_address(data: &str) -> Option<String> {
    u64::from_str_radix(data, 16).ok().map(|a| format!("{a:x}"))
}

fn snapshot(dir: &Path) -> Option<Raw> {
    let reply = request(dir, SNAPSHOT)?;
    let raw: Vec<String> = reply.split(BATCH_DELIMITER).map(str::to_owned).collect();
    raw.try_into().ok()
}

/// The `hypr-events` thread's lines. Dropping it ends the thread: its connection is shut down,
/// a wait for the socket or before a reconnect woken, and the thread joined.
struct Events {
    rx: Receiver<Option<String>>,
    stop: Arc<crate::svc::Stop>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl std::ops::Deref for Events {
    type Target = Receiver<Option<String>>;
    fn deref(&self) -> &Self::Target {
        &self.rx
    }
}

impl Drop for Events {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// What `dispatch` warns of, like Quickshell's HyprlandIpc::dispatch: no answer, or
/// Hyprland's reply when it is not `ok` (an unknown dispatcher, a Lua error).
fn dispatch_error(r: Result<String, IpcError>) -> Option<String> {
    match r {
        Ok(r) if r.trim() == "ok" => None,
        Ok(r) => Some(format!(" with error {:?}", r.trim())),
        Err(e) => Some(format!(": {e:?}")),
    }
}

/// Events after which the singleton's state (monitors, workspaces, toplevels, focus,
/// `lastIpcObject`) may differ: what Quickshell's HyprlandIpc::onEvent and its
/// `refreshMonitors`/`refreshWorkspaces`/`refreshToplevels` follow, and the `j/clients` fields
/// that change with no event of their own (`floating`, `pinned`, `grouped`, ...). The rest
/// (`activelayout`, `submap`, `bell`, layers, screencast, `custom`, ...) are only `rawEvent`s.
fn changes_state(name: &str) -> bool {
    matches!(
        name,
        "configreloaded"
            | "workspace"
            | "workspacev2"
            | "focusedmon"
            | "focusedmonv2"
            | "activewindow"
            | "activewindowv2"
            | "fullscreen"
            | "monitorremoved"
            | "monitorremovedv2"
            | "monitoradded"
            | "monitoraddedv2"
            | "createworkspace"
            | "createworkspacev2"
            | "destroyworkspace"
            | "destroyworkspacev2"
            | "moveworkspace"
            | "moveworkspacev2"
            | "renameworkspace"
            | "changeworkspaceid"
            | "activespecial"
            | "activespecialv2"
            | "openwindow"
            | "closewindow"
            | "movewindow"
            | "movewindowv2"
            | "windowtitle"
            | "windowtitlev2"
            | "changefloatingmode"
            | "urgent"
            | "pin"
            | "minimized"
            | "togglegroup"
            | "moveintogroup"
            | "moveoutofgroup"
    )
}

/// Two JSON values (`JSON.parse` results) with the same content.
fn same_json(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Obj(x), Value::Obj(y)) => {
            let (x, y) = (x.borrow(), y.borrow());
            match (&x.array, &y.array) {
                (Some(p), Some(q)) => {
                    p.len() == q.len() && p.iter().zip(q).all(|(p, q)| same_json(p, q))
                }
                (None, None) => {
                    x.props.len() == y.props.len()
                        && x.props
                            .iter()
                            .all(|(k, v)| y.props.get(k).is_some_and(|w| same_json(v, w)))
                }
                _ => false,
            }
        }
        _ => a.strict_eq(b),
    }
}

/// `lastIpcObject` follows each snapshot, but only an object whose reply changed is written:
/// a fresh, equal object notified every binding on every workspace, monitor and toplevel.
fn write_ipc(w: &mut World, i: usize, v: Value) {
    if !w
        .read(i, "lastIpcObject")
        .is_some_and(|o| same_json(&o, &v))
    {
        w.write(i, "lastIpcObject", v);
    }
}

/// HyprlandIpc's event socket. Quickshell connects once and goes deaf when it closes, but
/// Hyprland also drops a client whose event queue overflows (a stalled shell), not only on exit:
/// reconnect straight away, sending `None` so the state gets refetched for the events missed in
/// between. While the socket is missing or stale the thread sleeps on an inotify watch of its
/// directory and connects when a socket is created or moved there, with no time limit (as sd-bus
/// `watch_bind` does). A refused connect is a stale socket: Hyprland binds and listens on
/// `.socket2.sock` in one step of `initManagers(STAGE_PRIORITY)` and creates `.socket.sock`, which
/// [`socket_dir`] requires, only later in `STAGE_LATE`, so a live instance never shows this thread
/// a socket2 that is bound but not listening yet. Nobody listens on a dead instance's socket, nor
/// on one whose socket2 broke (Hyprland closes it for good). With inotify unavailable (instance
/// or fd limit) there is the one attempt Quickshell makes.
fn listen(dir: &Path, h: &crate::svc::Handle) -> Events {
    let (tx, rx) = h.channel();
    let path = dir.join(".socket2.sock");
    let stop = Arc::new(crate::svc::Stop::default());
    let st = stop.clone();
    let thread = std::thread::Builder::new()
        .name("hypr-events".into())
        .spawn(move || {
            let conn: Arc<std::sync::Mutex<Option<UnixStream>>> = Arc::default();
            let c = conn.clone();
            let _shutdown = st.on_stop_owned(move || {
                if let Some(s) = c.lock().unwrap_or_else(|e| e.into_inner()).take() {
                    let _ = s.shutdown(std::net::Shutdown::Both);
                }
            });
            // Set up before the first attempt so a socket created in between is not missed.
            let mut watch = crate::sysbus::SocketWatch::new(&path);
            if let Some(w) = watch.as_mut() {
                w.stop_on(&st);
            }
            follow(
                || {
                    let s = UnixStream::connect(&path)?;
                    *conn.lock().unwrap_or_else(|e| e.into_inner()) = Some(s.try_clone()?);
                    // A stop before the clone was stored found nothing to shut down.
                    if st.load(Ordering::SeqCst) {
                        let _ = s.shutdown(std::net::Shutdown::Both);
                    }
                    Ok(s)
                },
                || {
                    !st.load(Ordering::SeqCst)
                        && watch.as_mut().is_some_and(crate::sysbus::SocketWatch::wait)
                },
                |d| !st.wait_timeout(d),
                &tx,
            );
        })
        .inspect_err(|e| elog::eprintln!("hyprland: cannot start the hypr-events thread: {e}"))
        .ok();
    Events { rx, stop, thread }
}

/// The pause before reconnecting to an event socket that closed after less than
/// [`RECONNECT_MAX`]: doubled on each such close, up to that. Hyprland closes a client it
/// cannot write to (a full buffer) and accepts it again at once; reconnecting without a pause
/// spun a core on a compositor that keeps doing so.
const RECONNECT_MIN: std::time::Duration = std::time::Duration::from_millis(100);
const RECONNECT_MAX: std::time::Duration = std::time::Duration::from_secs(5);

/// The event socket's connection loop: `connect` attempts a connection, `wait` blocks until a
/// socket appears at the path (false: never again).
fn follow<S: Read>(
    mut connect: impl FnMut() -> std::io::Result<S>,
    mut wait: impl FnMut() -> bool,
    mut pause: impl FnMut(std::time::Duration) -> bool,
    tx: &crate::svc::Sender<Option<String>>,
) {
    let mut connected_before = false;
    let mut warned = false;
    let mut delay = std::time::Duration::ZERO;
    loop {
        match connect() {
            Ok(s) => {
                let since = std::time::Instant::now();
                if connected_before {
                    elog::eprintln!("Reconnected to hyprland event socket.");
                    if tx.send(None).is_err() {
                        return;
                    }
                }
                connected_before = true;
                warned = false;
                // Titles are not guaranteed UTF-8 (X11 Latin-1 `WM_NAME`): decode lossily
                // instead of dropping the connection and the rest of the batch.
                let mut r = std::io::BufReader::new(s);
                let mut buf = Vec::new();
                loop {
                    buf.clear();
                    match r.read_until(b'\n', &mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                    if buf.last() == Some(&b'\n') {
                        buf.pop();
                    }
                    let l = String::from_utf8_lossy(&buf).into_owned();
                    if tx.send(Some(l)).is_err() {
                        return;
                    }
                }
                elog::eprintln!("Hyprland event socket disconnected.");
                delay = if since.elapsed() >= RECONNECT_MAX {
                    std::time::Duration::ZERO
                } else {
                    (delay * 2).clamp(RECONNECT_MIN, RECONNECT_MAX)
                };
                if !pause(delay) {
                    return;
                }
                continue;
            }
            Err(e) => {
                if !std::mem::replace(&mut warned, true) {
                    elog::eprintln!("Unable to connect to hyprland event socket: {e}");
                }
            }
        }
        if !wait() {
            return;
        }
    }
}

/// The original's `searchToggleReleaseInterrupt`: Super+key must not also toggle search on Super's release.
/// The legacy config sent it from `binditn = Super, catchall` and from binds on Super + each
/// mouse button and wheel direction; Hyprland 0.57's Lua catchall only works in submaps and never
/// fires when another bind matched, and no bind matches "any key while Super is held", so a key
/// hook in Hyprland's Lua state sends it instead. `input.keyboard.key` runs before the keybind
/// manager records the key, so `hl.is_key_down` sees the keys held before it, by keysym, resolved
/// through the same keymap as the `SUPER + SUPER_L` binds (layout and `kb_options` remaps
/// included). There is no such event for the mouse: binds on every button and wheel direction,
/// under any modifiers and passing the press on, ask the same question (Super + drag moves a
/// window, Super + wheel changes workspace: neither is a search). `configreloaded` resets the Lua
/// state and the binds, so it is sent again then; sent to a state that has them, it replaces them.
const SUPER_INTERRUPT: &str = "eval if __ri_superIntr then __ri_superIntr:remove() end; for _, b in ipairs(__ri_superIntrBinds or {}) do pcall(b.remove, b) end; local function intr() if hl.is_key_down(\"Super_L\") or hl.is_key_down(\"Super_R\") then hl.dispatch(hl.dsp.global(\"rustshell:searchToggleReleaseInterrupt\")) end end; __ri_superIntr = hl.on(\"input.keyboard.key\", function(_, _, st) if st == 1 then intr() end end); __ri_superIntrBinds = {}; for _, k in ipairs({\"mouse:272\", \"mouse:273\", \"mouse:274\", \"mouse:275\", \"mouse:276\", \"mouse:277\", \"mouse_up\", \"mouse_down\", \"mouse_left\", \"mouse_right\"}) do table.insert(__ri_superIntrBinds, hl.bind(k, intr, { ignore_mods = true, non_consuming = true })) end";

fn send_super_interrupt(req: &Sender<Req>) {
    queue(
        req,
        Req::Dispatch(
            SUPER_INTERRUPT.into(),
            "Super release interrupt hook".into(),
        ),
    );
}

fn field(v: &Value, k: &str) -> Value {
    match v {
        Value::Obj(o) => o.borrow().props.get(k).cloned().unwrap_or_default(),
        _ => Value::Undefined,
    }
}

fn list(v: Option<Value>) -> Vec<Value> {
    match v {
        Some(Value::Obj(o)) => o.borrow().array.clone().unwrap_or_default(),
        _ => vec![],
    }
}

fn set_model(w: &mut World, owner: usize, prop: &str, items: Vec<Value>) {
    if let Some(Value::Item(m)) = w.read(owner, prop) {
        set_list(w, m, items);
    }
}

/// ObjectModel signals only real inserts/removes; an unchanged list re-runs nothing.
fn set_list(w: &mut World, m: usize, items: Vec<Value>) {
    let same = |o: &Value| match o {
        Value::Obj(o) => o.borrow().array.as_ref().is_some_and(|a| {
            a.len() == items.len() && a.iter().zip(&items).all(|(x, y)| x.strict_eq(y))
        }),
        _ => false,
    };
    if !w.read(m, "values").is_some_and(|o| same(&o)) {
        w.write(m, "values", Value::array(items));
    }
}

/// Apply a monitors/workspaces/clients reply.
fn refresh(w: &mut World, hy: usize, h: &mut Hypr, raw: &Raw) {
    let p = |s: &str| crate::json::parse(s).ok();
    let (Some(mons), Some(wss), Some(act), Some(clients), Some(active)) =
        (p(&raw[0]), p(&raw[1]), p(&raw[2]), p(&raw[3]), p(&raw[4]))
    else {
        // Taken again on the next state event, not on every poll for a ToplevelManager that
        // no snapshot has filled yet.
        h.tm = w.singletons.get("ToplevelManager").copied();
        return;
    };
    h.mons_requested = true;
    let act_addr = ws_address(&act);
    let mut seen = vec![];
    for ws in list(Some(wss)) {
        let Some(addr) = ws_address(&ws) else {
            continue;
        };
        seen.push(addr.clone());
        let i = *h
            .wss
            .entry(addr.clone())
            .or_insert_with(|| crate::types::native_singleton(w, "HyprlandWorkspace"));
        w.write(i, "id", Value::Num(ws_id(&addr) as f64));
        w.write(i, "address", Value::str(&addr));
        w.write(i, "name", field(&ws, "name"));
        w.write(
            i,
            "hasFullscreen",
            Value::Bool(field(&ws, "hasfullscreen").truthy()),
        );
        w.write(i, "focused", Value::Bool(act_addr.as_ref() == Some(&addr)));
        write_ipc(w, i, ws);
    }
    for (addr, i) in h.wss.clone() {
        if !seen.contains(&addr) {
            h.wss.remove(&addr);
            w.destroy(i);
        }
    }
    let mut seen = vec![];
    let mut focused = Value::Null;
    let mut by_name = BTreeMap::new();
    let mut active_of = BTreeMap::new();
    for m in list(Some(mons)) {
        let id = field(&m, "id").to_number() as i64;
        seen.push(id);
        let name = field(&m, "name").to_js_string();
        let i = *h.mons.entry(id).or_insert_with(|| {
            let early = w
                .hypr_early
                .clone()
                .into_iter()
                .position(|e| w.read(e, "name").is_some_and(|v| v.to_js_string() == name));
            match early {
                Some(p) => w.hypr_early.remove(p),
                None => crate::types::native_singleton(w, "HyprlandMonitor"),
            }
        });
        for k in ["name", "description", "x", "y", "width", "height", "scale"] {
            w.write(i, k, field(&m, k));
        }
        w.write(i, "id", Value::Num(id as f64));
        let foc = field(&m, "focused").truthy();
        w.write(i, "focused", Value::Bool(foc));
        if foc {
            focused = Value::Item(i);
        }
        let aw = ws_address(&field(&m, "activeWorkspace"));
        let awi = aw
            .as_ref()
            .and_then(|a| h.wss.get(a))
            .map_or(Value::Null, |&x| Value::Item(x));
        if let Some(a) = aw {
            active_of.insert(a, i);
        }
        w.write(i, "activeWorkspace", awi);
        by_name.insert(field(&m, "name").to_js_string(), i);
        write_ipc(w, i, m);
    }
    for (id, i) in h.mons.clone() {
        if !seen.contains(&id) {
            h.mons.remove(&id);
            w.destroy(i);
        }
    }
    for i in std::mem::take(&mut w.hypr_early) {
        w.destroy(i);
    }
    for (addr, &i) in &h.wss {
        let mon = w
            .read(i, "lastIpcObject")
            .map(|o| field(&o, "monitor").to_js_string())
            .and_then(|n| by_name.get(&n).copied());
        w.write(i, "monitor", mon.map_or(Value::Null, Value::Item));
        w.write(i, "active", Value::Bool(active_of.contains_key(addr)));
    }
    let wsv = ws_order(h).into_iter().map(Value::Item).collect();
    let mv = h.mons.values().map(|&i| Value::Item(i)).collect();
    set_model(w, hy, "workspaces", wsv);
    set_model(w, hy, "monitors", mv);
    w.write(hy, "focusedMonitor", focused);
    let fw = act_addr
        .and_then(|a| h.wss.get(&a))
        .map_or(Value::Null, |&x| Value::Item(x));
    w.write(hy, "focusedWorkspace", fw);
    toplevels(w, hy, h, clients, active);
}

/// A workspace object's identity: Hyprland 0.57+ `address` ("1", "special:magic", "name:x"),
/// or the numeric `id` older versions send instead (as Quickshell does).
fn ws_address(ws: &Value) -> Option<String> {
    match field(ws, "address") {
        Value::Str(a) if !a.is_empty() => Some(a.to_string()),
        _ => match field(ws, "id") {
            Value::Num(n) if n.is_finite() => Some((n as i64).to_string()),
            _ => None,
        },
    }
}

/// `HyprlandWorkspace.id`: the address as an integer, else -1 (Quickshell's binding).
fn ws_id(addr: &str) -> i64 {
    addr.parse().unwrap_or(-1)
}

/// `Hyprland.workspaces` order: by id like Qt's sorted model, then by address.
fn ws_order(h: &Hypr) -> Vec<usize> {
    let mut v: Vec<(i64, &String, usize)> = h.wss.iter().map(|(a, &i)| (ws_id(a), a, i)).collect();
    v.sort();
    v.into_iter().map(|t| t.2).collect()
}

/// Toplevels from `clients`/`activewindow`, standing in for wlr-foreign-toplevel
/// (`ToplevelManager`) and Rustshell.Hyprland's toplevel objects.
fn toplevels(w: &mut World, hy: usize, h: &mut Hypr, clients: Value, active: Value) {
    let addr = |v: &Value| {
        let a = field(v, "address").to_js_string();
        a.strip_prefix("0x").unwrap_or(&a).to_string()
    };
    let act = matches!(field(&active, "address"), Value::Str(_)).then(|| addr(&active));
    let mut seen = vec![];
    let mut per_ws: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let mut active_pair = None;
    // (workspace address, window address) of each toplevel.
    let mut placed = vec![];
    let mons: BTreeMap<i64, usize> = h.mons.clone();
    for c in list(Some(clients)) {
        if !field(&c, "mapped").truthy() {
            continue;
        }
        let a = addr(&c);
        let (t, ht) = *h.tops.entry(a.clone()).or_insert_with(|| {
            let t = crate::types::native_singleton(w, "Toplevel");
            let ht = crate::types::native_singleton(w, "HyprlandToplevel");
            (t, ht)
        });
        let activated = act.as_deref() == Some(a.as_str());
        // HyprlandToplevel::onActivatedChanged: a change of `activated` ends the urgency.
        if w.read(ht, "activated").is_some_and(|v| v.truthy()) != activated {
            h.urgent.remove(&a);
        }
        let fs = field(&c, "fullscreen").to_number() as i64;
        let title = field(&c, "title");
        w.write(t, "appId", field(&c, "class"));
        w.write(t, "title", title.clone());
        w.write(t, "activated", Value::Bool(activated));
        w.write(t, "maximized", Value::Bool(fs & 1 != 0));
        w.write(t, "fullscreen", Value::Bool(fs & 2 != 0));
        w.write(t, "minimized", Value::Bool(false));
        w.write(t, "HyprlandToplevel", Value::Item(ht));
        let ws = ws_address(&field(&c, "workspace")).unwrap_or_default();
        let wsi = h.wss.get(&ws).map_or(Value::Null, |&x| Value::Item(x));
        let mon = field(&c, "monitor").to_number() as i64;
        let mi = mons.get(&mon).map_or(Value::Null, |&x| Value::Item(x));
        w.write(ht, "address", Value::str(&a));
        w.write(ht, "handle", Value::Item(ht));
        w.write(ht, "wayland", Value::Item(t));
        w.write(ht, "title", title);
        w.write(ht, "activated", Value::Bool(activated));
        w.write(ht, "workspace", wsi);
        w.write(ht, "monitor", mi);
        write_ipc(w, ht, c);
        per_ws.entry(ws.clone()).or_default().push(Value::Item(ht));
        placed.push((ws, a.clone()));
        if activated {
            active_pair = Some((t, ht));
        }
        seen.push(a);
    }
    for (a, (t, ht)) in h.tops.clone() {
        if !seen.contains(&a) {
            h.tops.remove(&a);
            w.destroy(t);
            w.destroy(ht);
        }
    }
    // HyprlandWorkspace::updateUrgent: a workspace is urgent while one of its toplevels is,
    // and the focused workspace ends the urgency of all of its toplevels.
    for (addr, &i) in &h.wss {
        let here = || placed.iter().filter(|p| p.0 == *addr).map(|p| &p.1);
        let mut urgent = here().any(|a| h.urgent.contains(a));
        if urgent && w.read(i, "focused").is_some_and(|v| v.truthy()) {
            for a in here() {
                h.urgent.remove(a);
            }
            urgent = false;
        }
        w.write(i, "urgent", Value::Bool(urgent));
    }
    for a in &seen {
        w.write(h.tops[a].1, "urgent", Value::Bool(h.urgent.contains(a)));
    }
    let order: Vec<(usize, usize)> = seen.iter().map(|a| h.tops[a]).collect();
    for (addr, &i) in &h.wss {
        let model = match w.read(i, "toplevels") {
            Some(Value::Item(m)) => m,
            _ => {
                let m = w.new_inst("ObjectModel", None);
                // Owned by its workspace, as Quickshell's model is its QObject child.
                w.owned.entry(i).or_default().push(m);
                w.declare(m, "values", "list");
                w.write(i, "toplevels", Value::Item(m));
                m
            }
        };
        let v = per_ws.remove(addr).unwrap_or_default();
        set_list(w, model, v);
    }
    set_model(
        w,
        hy,
        "toplevels",
        order.iter().map(|p| Value::Item(p.1)).collect(),
    );
    w.write(
        hy,
        "activeToplevel",
        active_pair.map_or(Value::Null, |p| Value::Item(p.1)),
    );
    h.tm = w.singletons.get("ToplevelManager").copied();
    if let Some(tm) = h.tm {
        set_model(
            w,
            tm,
            "toplevels",
            order.iter().map(|p| Value::Item(p.0)).collect(),
        );
        w.write(
            tm,
            "activeToplevel",
            active_pair.map_or(Value::Null, |p| Value::Item(p.0)),
        );
    }
}

/// `Hyprland.monitorFor(screen)`.
pub fn monitor_for(w: &mut World, screen: Option<&Value>) -> Value {
    let name = match screen {
        Some(Value::Item(s)) => w.read(*s, "name").map(|v| v.to_js_string()),
        Some(v @ Value::Obj(_)) => Some(field(v, "name").to_js_string()),
        _ => None,
    };
    let Some(name) = name else {
        return Value::Null;
    };
    // Depend on the monitor set, so a binding whose monitor is added (or removed) later re-runs.
    if let Some(&hy) = w.singletons.get("Hyprland")
        && let Some(Value::Item(m)) = w.read(hy, "monitors")
    {
        w.read(m, "values");
    }
    let (mons, requested): (Vec<usize>, bool) = match w.hypr.as_ref() {
        Some(h) => (h.mons.values().copied().collect(), h.mons_requested),
        None => (vec![], false),
    };
    let found = mons
        .into_iter()
        .chain(w.hypr_early.clone())
        .find(|&m| w.read(m, "name").is_some_and(|v| v.to_js_string() == name));
    if let Some(m) = found {
        return Value::Item(m);
    }
    if requested {
        return Value::Null;
    }
    // HyprlandIpc::findMonitorByName(name, createIfMissing): early init before the first reply.
    let m = crate::types::native_singleton(w, "HyprlandMonitor");
    w.write(m, "id", Value::Num(-1.0));
    w.write(m, "name", Value::str(&name));
    w.hypr_early.push(m);
    Value::Item(m)
}

/// Apply a reply of the `requests` worker; returns whether anything changed.
fn apply(w: &mut World, hy: usize, h: &mut Hypr, reply: Reply) -> bool {
    match reply {
        Reply::UsingLua(lua) => {
            w.write(hy, "usingLua", Value::Bool(lua));
            true
        }
        Reply::Snapshot(raw) => {
            h.pending = false;
            // A snapshot that failed (Hyprland exiting, a reply cut short) is taken again on
            // the next event, which marks the state dirty.
            let Some(raw) = raw else {
                return false;
            };
            refresh(w, hy, h, &raw);
            true
        }
    }
}

impl Runtime {
    /// The Hyprland singleton has a state refresh out or due (tests wait for it to settle).
    pub fn hyprland_refreshing(&self) -> bool {
        self.world
            .borrow()
            .hypr
            .as_ref()
            .is_some_and(|h| h.pending || h.dirty)
    }

    /// Drive the Hyprland singleton (if instantiated); returns whether anything changed.
    pub fn poll_hyprland(&mut self, dispatches: Vec<String>) -> bool {
        let Some(&hy) = self.world.borrow().singletons.get("Hyprland") else {
            return false;
        };
        let mut w = self.world.borrow_mut();
        if w.hypr.is_none() {
            // Connect only to an instance that is up: `.socket.sock` alone may be bound but not
            // listening yet, and a snapshot refused then would leave the singleton empty until
            // Hyprland's first event.
            let dir = match w.hypr_wait.as_ref() {
                Some(wait) => wait.ready(),
                None => match instance_dir() {
                    Some(d) if instance_up(&d) => Some(d),
                    d => {
                        let h = w.waker.handle(Svc::Hypr);
                        w.hypr_wait = d.map(|d| wait(d, &h));
                        None
                    }
                },
            };
            let Some(dir) = dir else {
                return false;
            };
            w.hypr_wait = None;
            let rs = dir.join(".socket.sock").display().to_string();
            let es = dir.join(".socket2.sock").display().to_string();
            w.write(hy, "requestSocketPath", Value::str(&rs));
            w.write(hy, "eventSocketPath", Value::str(&es));
            let h = w.waker.handle(Svc::Hypr);
            let (req, replies) = requests(&dir, &h);
            queue(&req, Req::Status);
            send_super_interrupt(&req);
            w.hypr = Some(Hypr {
                rx: listen(&dir, &h),
                req,
                replies,
                pending: false,
                mons: BTreeMap::new(),
                wss: BTreeMap::new(),
                tops: BTreeMap::new(),
                tm: None,
                dirty: true,
                mons_requested: false,
                urgent: BTreeSet::new(),
            });
        }
        let mut h = w.hypr.take().unwrap();
        if h.tm.is_none() && w.singletons.contains_key("ToplevelManager") {
            h.dirty = true;
        }
        let mut changed = false;
        while let Ok(reply) = h.replies.try_recv() {
            changed |= apply(&mut w, hy, &mut h, reply);
        }
        for d in dispatches {
            let t = d.trim_start();
            // `hl.config(...)` (Overview's cursor.no_warps) is no dispatcher: Hyprland 0.57's
            // `dispatch` applies it, then fails `hl.dispatch(nil)`; evaluate it directly.
            let cmd = if t.starts_with("hl.") && !t.starts_with("hl.dsp.") {
                format!("eval {d}")
            } else if t.starts_with("hl.") {
                format!("eval return hl.dispatch({d})")
            } else {
                format!("dispatch {d}")
            };
            if crate::log_on!("ri::tracew") {
                tracing::debug!(target: "ri::tracew", "hypr dispatch t={}", crate::native::trace_ms());
            }
            queue(&h.req, Req::Dispatch(cmd, d));
        }
        let lines: Vec<Option<String>> = std::iter::from_fn(|| h.rx.try_recv().ok()).collect();
        for l in lines {
            let Some(l) = l else {
                // Reconnected: Hyprland may have restarted (another config provider, a new Lua
                // state without the hook) or dropped events while it could not write to us.
                queue(&h.req, Req::Status);
                send_super_interrupt(&h.req);
                h.dirty = true;
                continue;
            };
            let (name, data) = l.split_once(">>").unwrap_or((&l, ""));
            if crate::log_on!("ri::tracew") {
                tracing::debug!(target: "ri::tracew", "hypr event {name} t={}", crate::native::trace_ms());
            }
            let ev = crate::eval::new_obj();
            crate::eval::set_own(&ev, "name", Value::str(name));
            crate::eval::set_own(&ev, "data", Value::str(data));
            crate::native::emit(&mut w, hy, "rawEvent", &["event"], vec![Value::Obj(ev)]);
            if name == "configreloaded" {
                send_super_interrupt(&h.req);
            }
            // HyprlandIpc::onEvent: `urgent` marks the window, also one that `openwindow` has
            // not announced yet; the next snapshot shows it on the toplevel and its workspace.
            match name {
                "urgent" => h.urgent.extend(event_address(data)),
                "closewindow" => {
                    if let Some(a) = event_address(data) {
                        h.urgent.remove(&a);
                    }
                }
                _ => {}
            }
            if changes_state(name) {
                h.dirty = true;
            }
            changed = true;
        }
        // One refresh out at a time; events during it ask for the next one.
        if !h.pending && std::mem::take(&mut h.dirty) && queue(&h.req, Req::Refresh) {
            h.pending = true;
            // The request thread answers every refresh, or ends and so disconnects: a
            // reply always comes, however long Hyprland takes.
            if SYNC.load(Ordering::Relaxed) {
                while h.pending
                    && let Ok(reply) = h.replies.recv()
                {
                    changed |= apply(&mut w, hy, &mut h, reply);
                }
                h.pending = false;
            }
        }
        w.hypr = Some(h);
        changed
    }
}

#[cfg(test)]
mod tests {
    use crate::value::Value;
    use std::io::Write;
    use std::os::fd::FromRawFd;
    use std::os::unix::net::UnixListener;
    use std::path::Path;

    #[test]
    fn non_utf8_event_line_keeps_the_socket() {
        // An X11 window with a Latin-1 title: `lines()` failed on it, dropped the connection
        // and every event Hyprland had already written after it.
        let dir = std::env::temp_dir().join(format!("ri-hypr-utf8-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let sock = dir.join(".socket2.sock");
        let _ = std::fs::remove_file(&sock);
        let l = UnixListener::bind(&sock).unwrap();
        let waker = crate::svc::Waker::default();
        let rx = super::listen(&dir, &waker.handle(crate::svc::Svc::Hypr));
        let (mut s, _) = l.accept().unwrap();
        s.write_all(b"windowtitlev2>>a1,Caf\xe9\nworkspace>>2\n")
            .unwrap();
        let got: Vec<Option<String>> = (0..2).map(|_| rx.recv().unwrap()).collect();
        assert_eq!(
            got,
            [
                Some("windowtitlev2>>a1,Caf\u{fffd}".to_string()),
                Some("workspace>>2".to_string())
            ]
        );
        std::fs::remove_file(&sock).unwrap();
        drop(s);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn instance_is_up_once_its_lock_exists() {
        // Hyprland binds `.socket.sock` before listen(2) and writes the lock after both.
        let dir = std::env::temp_dir().join(format!("ri-hypr-up-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let _l = UnixListener::bind(dir.join(".socket.sock")).unwrap();
        assert!(!super::instance_up(&dir));
        std::fs::write(dir.join(super::LOCK), "1\nwayland-1\n").unwrap();
        assert!(super::instance_up(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wait_thread_ends_when_its_wait_is_dropped() {
        // A World dropped while its Hyprland is not up (a test, a `qs -p` window that exits)
        // left the thread blocked on inotify, with its watches and a clone of the waker.
        let rt = std::env::temp_dir().join(format!("ri-hypr-wait-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&rt);
        std::fs::create_dir_all(&rt).unwrap();
        let waker = crate::svc::Waker::default();
        let wait = super::wait(rt.join("hypr/NONE"), &waker.handle(crate::svc::Svc::Hypr));
        drop(wait);
        // The thread's sender held the other clone until the thread ended.
        assert_eq!(waker.clones(), 1);
        let _ = std::fs::remove_dir_all(&rt);
    }

    #[test]
    fn removed_workspaces_take_their_objects_along() {
        // A workspace's `toplevels` model had no owner: each workspace that came and went
        // (moving to an empty one and back) left it alive.
        let mut w = crate::runtime::World::default();
        let hy = crate::types::native_singleton(&mut w, "Hyprland");
        let (req, _requests) = std::sync::mpsc::channel();
        let (_events, rx) = std::sync::mpsc::channel();
        let rx = super::Events {
            rx,
            stop: Default::default(),
            thread: None,
        };
        let (_snapshots, replies) = std::sync::mpsc::channel();
        let mut h = super::Hypr {
            rx,
            req,
            replies,
            pending: false,
            mons: Default::default(),
            wss: Default::default(),
            tops: Default::default(),
            tm: None,
            dirty: false,
            mons_requested: false,
            urgent: Default::default(),
        };
        let ws =
            |id: u32| format!(r#"{{"address":"{id}","id":{id},"name":"{id}","monitor":"DP-1"}}"#);
        let raw = |wss: &[u32]| -> super::Raw {
            let list: Vec<String> = wss.iter().map(|&i| ws(i)).collect();
            [
                r#"[{"id":0,"name":"DP-1","activeWorkspace":{"address":"1","id":1,"name":"1"},"focused":true}]"#.into(),
                format!("[{}]", list.join(",")),
                ws(1),
                "[]".into(),
                "{}".into(),
            ]
        };
        let live =
            |w: &crate::runtime::World| (0..w.items.len()).filter(|i| !w.dead.contains(i)).count();
        super::refresh(&mut w, hy, &mut h, &raw(&[1]));
        let before = live(&w);
        for _ in 0..5 {
            super::refresh(&mut w, hy, &mut h, &raw(&[1, 2]));
            let two = h.wss["2"];
            assert!(matches!(w.read(two, "toplevels"), Some(Value::Item(_))));
            super::refresh(&mut w, hy, &mut h, &raw(&[1]));
        }
        assert_eq!(live(&w), before);
    }

    #[test]
    fn snapshot_is_one_batch_request() {
        let dir = std::env::temp_dir().join(format!("ri-hypr-snap-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let sock = dir.join(".socket.sock");
        let _ = std::fs::remove_file(&sock);
        let l = UnixListener::bind(&sock).unwrap();
        let srv = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut buf = [0u8; 256];
            let n = std::io::Read::read(&mut s, &mut buf).unwrap_or(0);
            let cmd = String::from_utf8_lossy(&buf[..n]).to_string();
            // As Hyprland's `joinBatchReplies`: the replies with the delimiter between them.
            let replies = [
                r#"[{"id":0,"name":"DP-1","activeWorkspace":{"id":1,"name":"1"}}]"#,
                "[{\n    \"id\": 1,\n    \"name\": \"1\"\n},{\n    \"id\": 2,\n    \"name\": \"2\"\n}]",
                r#"{"id":1,"name":"1"}"#,
                r#"[{"address":"0xa1","mapped":true,"workspace":{"id":2,"name":"2"}}]"#,
                "{}",
            ];
            let _ = s.write_all(replies.join("\n\n\n").as_bytes());
            drop(s);
            // The whole snapshot took that one connection.
            l.set_nonblocking(true).unwrap();
            assert!(l.accept().is_err());
            cmd
        });
        let raw = super::snapshot(&dir).unwrap();
        let cmd = srv.join().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            cmd,
            "[[BATCH]]j/monitors;j/workspaces;j/activeworkspace;j/clients;j/activewindow"
        );
        assert!(raw[0].contains("DP-1"), "{raw:?}");
        assert!(raw[1].contains("\"id\": 2"), "{raw:?}");
        assert_eq!(raw[2], r#"{"id":1,"name":"1"}"#);
        assert!(raw[3].contains("0xa1"), "{raw:?}");
        assert_eq!(raw[4], "{}");
    }

    #[test]
    fn snapshot_rejects_a_short_batch_reply() {
        let dir = std::env::temp_dir().join(format!("ri-hypr-short-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let sock = dir.join(".socket.sock");
        let _ = std::fs::remove_file(&sock);
        let l = UnixListener::bind(&sock).unwrap();
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut buf = [0u8; 256];
            let _ = std::io::Read::read(&mut s, &mut buf);
            let _ = s.write_all(b"unknown request");
        });
        assert!(super::snapshot(&dir).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn request_reply_with_invalid_utf8_is_decoded_lossily() {
        let dir = std::env::temp_dir().join(format!("ri-hypr-req-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let sock = dir.join(".socket.sock");
        let _ = std::fs::remove_file(&sock);
        let l = UnixListener::bind(&sock).unwrap();
        let srv = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut req = [0u8; 64];
            let n = std::io::Read::read(&mut s, &mut req).unwrap();
            assert_eq!(&req[..n], b"j/clients");
            s.write_all(b"[{\"title\": \"caf\xe9\"}]").unwrap();
        });
        let got = super::request(&dir, "j/clients");
        srv.join().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let got = got.expect("reply dropped");
        let v: serde_json::Value = serde_json::from_str(&got).unwrap();
        assert_eq!(v[0]["title"], "caf\u{fffd}");
    }

    /// Hyprland answers `.socket.sock` when its event loop gets to the request: a large
    /// `j/clients` or a modeset can take longer than any fixed deadline, and a reply given
    /// up on was a lost state refresh.
    #[test]
    fn request_waits_for_a_slow_reply() {
        let dir = std::env::temp_dir().join(format!("ri-hypr-slow-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let sock = dir.join(".socket.sock");
        let _ = std::fs::remove_file(&sock);
        let l = UnixListener::bind(&sock).unwrap();
        let (tx, asked) = std::sync::mpsc::channel();
        let (release, held) = std::sync::mpsc::channel::<()>();
        let srv = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut req = [0u8; 64];
            let n = std::io::Read::read(&mut s, &mut req).unwrap();
            assert_eq!(&req[..n], b"j/monitors");
            tx.send(()).unwrap();
            // The reply comes only when the test lets it: until then the client waits.
            held.recv().unwrap();
            // A client that gave up has closed its end: the reply is lost (EPIPE).
            s.write_all(b"[]")
        });
        let d = dir.clone();
        let client = std::thread::spawn(move || super::request(&d, "j/monitors"));
        asked.recv().unwrap();
        release.send(()).unwrap();
        srv.join()
            .unwrap()
            .expect("the client gave up on the reply");
        let got = client.join().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(got.as_deref(), Some("[]"));
    }

    /// Bind and listen at a scratch name, then rename onto `sock`: the socket is listening
    /// before it appears at the path (one IN_MOVED_TO), so no connect sees it bound but not
    /// listening.
    fn publish(sock: &Path) -> UnixListener {
        let tmp = sock.with_extension("new");
        let _ = std::fs::remove_file(&tmp);
        let l = UnixListener::bind(&tmp).unwrap();
        std::fs::rename(&tmp, sock).unwrap();
        l
    }

    #[test]
    fn refused_connect_waits_for_the_next_socket_event() {
        // A refused connect is a stale socket: the next attempt follows the next inotify
        // event, never a timer.
        let log = std::cell::RefCell::new(Vec::new());
        let waker = crate::svc::Waker::default();
        let (tx, _rx) = waker.handle(crate::svc::Svc::Hypr).channel();
        let mut events = 2;
        super::follow(
            || -> std::io::Result<std::io::Empty> {
                log.borrow_mut().push("connect");
                Err(std::io::ErrorKind::ConnectionRefused.into())
            },
            || {
                log.borrow_mut().push("wait");
                events -= 1;
                events >= 0
            },
            |_| true,
            &tx,
        );
        assert_eq!(
            *log.borrow(),
            ["connect", "wait", "connect", "wait", "connect", "wait"]
        );
    }

    #[test]
    fn event_socket_reconnects_and_waits_out_a_missing_socket() {
        // Hyprland drops a client whose event queue overflows; Quickshell then went deaf.
        let dir = std::env::temp_dir().join(format!("ri-hypr-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let sock = dir.join(".socket2.sock");
        let _ = std::fs::remove_file(&sock);
        let l = UnixListener::bind(&sock).unwrap();
        let waker = crate::svc::Waker::default();
        let rx = super::listen(&dir, &waker.handle(crate::svc::Svc::Hypr));
        let (mut s, _) = l.accept().unwrap();
        s.write_all(b"workspace>>2\nactivewindow>>kitty,~\n")
            .unwrap();
        drop(s);
        let (mut s, _) = l.accept().unwrap();
        s.write_all(b"workspace>>3\n").unwrap();
        // The socket goes away: the thread waits on inotify until the next one appears.
        std::fs::remove_file(&sock).unwrap();
        drop(s);
        drop(l);
        let got: Vec<Option<String>> = (0..4).map(|_| rx.recv().unwrap()).collect();
        let l = publish(&sock);
        let (mut s, _) = l.accept().unwrap();
        s.write_all(b"workspace>>4\n").unwrap();
        let more: Vec<Option<String>> = (0..2).map(|_| rx.recv().unwrap()).collect();
        let some = |s: &str| Some(s.to_string());
        assert_eq!(
            got,
            [
                some("workspace>>2"),
                some("activewindow>>kitty,~"),
                None,
                some("workspace>>3")
            ]
        );
        assert_eq!(more, [None, some("workspace>>4")]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn socket_bound_but_not_listening_waits_for_the_next_socket() {
        use std::os::unix::ffi::OsStrExt;
        let dir = std::env::temp_dir().join(format!("ri-hypr-bind-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let sock = dir.join(".socket2.sock");
        let _ = std::fs::remove_file(&sock);
        // bind(2) without listen(2): the file exists and connect(2) is refused.
        // SAFETY: socket(2) takes plain integers.
        let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0) };
        assert!(fd >= 0);
        // SAFETY: `fd` is the socket created above, open and owned by nothing else.
        let bound = unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) };
        // SAFETY: sockaddr_un is integers and a byte array, for which all zeros is valid.
        let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
        for (d, s) in addr.sun_path.iter_mut().zip(sock.as_os_str().as_bytes()) {
            *d = *s as libc::c_char;
        }
        let len = std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t;
        // SAFETY: `addr` is a live sockaddr_un and `len` its size.
        assert_eq!(unsafe { libc::bind(fd, (&raw const addr).cast(), len) }, 0);
        let waker = crate::svc::Waker::default();
        let rx = super::listen(&dir, &waker.handle(crate::svc::Svc::Hypr));
        let l = publish(&sock);
        let (mut s, _) = l.accept().unwrap();
        s.write_all(b"workspace>>6\n").unwrap();
        assert_eq!(rx.recv().unwrap(), Some("workspace>>6".to_string()));
        drop(bound);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stale_event_socket_waits_for_a_new_one() {
        // A socket file nobody listens on refuses connections; the next socket put at the path
        // is connected to as soon as it appears.
        let dir = std::env::temp_dir().join(format!("ri-hypr-stale-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let sock = dir.join(".socket2.sock");
        let _ = std::fs::remove_file(&sock);
        drop(UnixListener::bind(&sock).unwrap());
        let waker = crate::svc::Waker::default();
        let rx = super::listen(&dir, &waker.handle(crate::svc::Svc::Hypr));
        let l = publish(&sock);
        let (mut s, _) = l.accept().unwrap();
        s.write_all(b"workspace>>5\n").unwrap();
        assert_eq!(rx.recv().unwrap(), Some("workspace>>5".to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reconnects_to_a_peer_that_closes_at_once_after_a_pause() {
        // Hyprland closes a client it cannot write to and accepts it again at once:
        // reconnecting straight away spun a core for as long as that went on.
        let dir = std::env::temp_dir().join(format!("ri-hypr-spin-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let sock = dir.join(".socket2.sock");
        let _ = std::fs::remove_file(&sock);
        let l = UnixListener::bind(&sock).unwrap();
        let waker = crate::svc::Waker::default();
        let rx = super::listen(&dir, &waker.handle(crate::svc::Svc::Hypr));
        let at: Vec<std::time::Instant> = (0..4)
            .map(|_| {
                drop(l.accept().unwrap());
                std::time::Instant::now()
            })
            .collect();
        // Pauses of 100, 200 and 400 ms between the four connections.
        let spent = at[3] - at[0];
        assert!(spent >= std::time::Duration::from_millis(700), "{spent:?}");
        drop(rx);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The pause doubles from [`RECONNECT_MIN`](super::RECONNECT_MIN) and stops growing at
    /// [`RECONNECT_MAX`](super::RECONNECT_MAX), however long the peer keeps closing at once.
    #[test]
    fn the_pause_before_reconnecting_doubles_up_to_its_longest() {
        let pauses = std::cell::RefCell::new(Vec::new());
        let waker = crate::svc::Waker::default();
        let (tx, _rx) = waker.handle(crate::svc::Svc::Hypr).channel();
        super::follow(
            // A connection that is at its end as soon as it is made.
            || Ok(std::io::empty()),
            || unreachable!("no connect fails"),
            |d| {
                pauses.borrow_mut().push(d.as_millis());
                pauses.borrow().len() < 9
            },
            &tx,
        );
        assert_eq!(
            *pauses.borrow(),
            [100, 200, 400, 800, 1600, 3200, 5000, 5000, 5000]
        );
    }

    #[test]
    fn event_thread_ends_when_its_receiver_is_dropped() {
        // A Hypr dropped (a World of a test, a `qs -p` window that exits) left the thread
        // blocked on its read or on inotify, with the connection and a clone of the waker.
        let dir = std::env::temp_dir().join(format!("ri-hypr-end-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let waker = crate::svc::Waker::default();
        let rx = super::listen(&dir, &waker.handle(crate::svc::Svc::Hypr));
        drop(rx);
        assert_eq!(waker.clones(), 1, "waiting for the socket");
        let sock = dir.join(".socket2.sock");
        let l = UnixListener::bind(&sock).unwrap();
        let rx = super::listen(&dir, &waker.handle(crate::svc::Svc::Hypr));
        let (mut s, _) = l.accept().unwrap();
        drop(rx);
        assert_eq!(waker.clones(), 1, "connected");
        let mut b = [0u8; 1];
        assert_eq!(std::io::Read::read(&mut s, &mut b).unwrap(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn requests_behind_an_unread_refresh_still_go_out() {
        // The worker ended at the first snapshot nobody read (its Hypr gone): the requests
        // queued behind it were never sent and `drain` waited for them forever.
        let dir = std::env::temp_dir().join(format!("ri-hypr-unread-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let l = UnixListener::bind(dir.join(".socket.sock")).unwrap();
        let waker = crate::svc::Waker::default();
        let (req, replies) = super::requests(&dir, &waker.handle(crate::svc::Svc::Hypr));
        drop(replies);
        let dispatch = |c: &str| super::Req::Dispatch(format!("dispatch {c}"), c.to_string());
        for r in [
            super::Req::Refresh,
            dispatch("exec a"),
            super::Req::Refresh,
            dispatch("exec b"),
        ] {
            assert!(super::queue(&req, r));
        }
        // Named in the output nextest shows when its slow-timeout ends a hang.
        eprintln!("waiting for the requests queued behind the unread snapshot");
        let got: Vec<String> = (0..3)
            .map(|_| {
                let (mut s, _) = l.accept().unwrap();
                let mut buf = [0u8; 4096];
                let n = std::io::Read::read(&mut s, &mut buf).unwrap();
                s.write_all(b"ok").unwrap();
                String::from_utf8_lossy(&buf[..n]).into_owned()
            })
            .collect();
        // The second snapshot is not taken: nobody would read it.
        assert_eq!(got, [super::SNAPSHOT, "dispatch exec a", "dispatch exec b"]);
        super::drain();
        drop(req);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The system refuses the Hyprland threads (the limit on threads is reached): each of the
    /// three starts returns, instead of panicking, what it returns without an instance. The
    /// wait never names a directory, the event stream is ended, and a request is reported
    /// as not sent and is not left counted as queued (`drain` returns). Each refusal is in the
    /// log. In a process of its own: the test reads what that process wrote to its stderr.
    #[test]
    fn refused_threads_leave_hyprland_as_without_an_instance() {
        const TEST: &str = concat!(
            module_path!(),
            "::refused_threads_leave_hyprland_as_without_an_instance"
        );
        if !crate::testenv::in_own_process(TEST) {
            crate::testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
            return;
        }
        let refused = |log: &str, thread: &str| {
            let line = format!("hyprland: cannot start the {thread} thread:");
            assert!(log.contains(&line), "stderr: {log:?}");
        };
        let dir = std::env::temp_dir().join(format!("ri-hypr-nothread-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let waker = crate::svc::Waker::default();
        let h = waker.handle(crate::svc::Svc::Hypr);
        let clones = waker.clones();
        crate::testenv::refuse_new_threads();
        let (wait, log) = crate::testenv::stderr_with(|| super::wait(dir.join("hypr/NONE"), &h));
        refused(&log, "hypr-wait");
        assert_eq!(wait.ready(), None);
        drop(wait);
        let (events, log) = crate::testenv::stderr_with(|| super::listen(&dir, &h));
        refused(&log, "hypr-events");
        assert_eq!(
            events.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Disconnected)
        );
        drop(events);
        let ((req, replies), log) = crate::testenv::stderr_with(|| super::requests(&dir, &h));
        refused(&log, "hypr-req");
        assert!(!super::queue(&req, super::Req::Refresh));
        assert_eq!(
            replies.try_recv().err(),
            Some(std::sync::mpsc::TryRecvError::Disconnected)
        );
        super::drain();
        drop((req, replies));
        // Every sender went with its thread's closure.
        assert_eq!(waker.clones(), clones);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
