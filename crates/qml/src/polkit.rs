//! Native `Rustshell.Services.Polkit`: a polkit authentication agent that speaks the agent
//! D-Bus protocol directly (zbus) and drives `polkit-agent-helper-1` the way libpolkit-agent-1's
//! `PolkitAgentSession` does, so neither glib nor libpolkit is needed. State machine mirrors
//! quickshell `src/services/polkit` (agentimpl.cpp / flow.cpp).
//!
//! The agent also takes the PIN codes / passkeys the BlueZ pairing agent (`crate::bluez_agent`)
//! needs typed: each becomes a flow in the same queue with no helper session (`iconName`
//! "bluetooth", a visible response), so the shell's polkit dialog asks for it. `submit()`
//! validates the text and answers bluetoothd; invalid text fails the attempt like a wrong
//! password, and bluetoothd cancelling closes the flow like polkitd cancelling.

use crate::bluez_agent::{Entry, Reply, Typed};
use crate::pam::{ChildRef, zero};
use crate::runtime::{Runtime, World};
use crate::svc::Stop;
use crate::value::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;
use zbus::MatchRule;
use zbus::blocking::Connection;
use zbus::message::{Header, Type as MsgType};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value as ZV};

const AUTHORITY: &str = "org.freedesktop.PolicyKit1";
const AUTHORITY_PATH: &str = "/org/freedesktop/PolicyKit1/Authority";
const AUTHORITY_IFACE: &str = "org.freedesktop.PolicyKit1.Authority";
const AGENT_IFACE: &str = "org.freedesktop.PolicyKit1.AuthenticationAgent";
const DEFAULT_PATH: &str = "/org/rustshell/PolkitAgent";
const HELPERS: &[&str] = &[
    "/usr/lib/polkit-1/polkit-agent-helper-1",
    "/usr/libexec/polkit-agent-helper-1",
    "/usr/lib/policykit-1/polkit-agent-helper-1",
    "/usr/libexec/polkit-1/polkit-agent-helper-1",
];
const HELPER_SOCKET: &str = "/run/polkit/agent-helper.socket";

type Subject = (String, HashMap<String, OwnedValue>);

/// polkit identity: (is group, uid/gid).
type Ident = (bool, u32);

/// Who waits for a request's answer.
enum Caller {
    /// The pending BeginAuthentication: `None` succeeds it, `Some(reason)` cancels it.
    Polkit(async_channel::Sender<Option<String>>),
    Bluez(Entry),
}

struct Req {
    call: Caller,
    action_id: String,
    message: String,
    icon_name: String,
    cookie: String,
    identities: Vec<Ident>,
}

enum BusEv {
    Registered(bool),
    Begin(Req),
    /// CancelAuthentication(cookie); answered with whether a request had it.
    Cancel(async_channel::Sender<bool>, String),
}

enum SessEv {
    Line(String),
    End,
}

struct Session {
    id: u64,
    input: Box<dyn Write + Send>,
    child: Option<ChildRef>,
    /// The socket-activated helper's connection. The reader thread holds a dup of it, so dropping
    /// `input` alone never closes it: the helper would wait for a response forever.
    sock: Option<std::os::unix::net::UnixStream>,
    /// The helper asked for a response (a `PAM_PROMPT_*` line).
    asked: bool,
}

impl Session {
    /// kill_helper(): SIGTERM the spawned helper (its reader thread reaps it) / close the socket.
    fn kill(self) {
        if let Some(c) = &self.child {
            c.signal(libc::SIGTERM);
        }
        if let Some(s) = &self.sock {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
    }
}

struct Identity {
    item: usize,
    ident: Ident,
    name: String,
}

struct Flow {
    item: usize,
    req: Req,
    identities: Vec<Identity>,
    selected: usize,
    session: Option<Session>,
    cancelled: bool,
}

pub struct Polkit {
    agent: usize,
    path: String,
    bus: Receiver<BusEv>,
    registered: bool,
    queue: VecDeque<Req>,
    flow: Option<Flow>,
    sess_tx: crate::svc::Sender<(u64, SessEv)>,
    sess_rx: Receiver<(u64, SessEv)>,
    /// PIN/passkey entries from the BlueZ pairing agent.
    bt: Receiver<Typed>,
    _dialog: crate::bluez_agent::DialogGuard,
    next_session: u64,
    /// Finished flows, destroyed once their signals were delivered.
    doomed: Vec<usize>,
    /// `authenticationRequestStarted` waits one settle after `isActive`/`flow` were set, so
    /// bindings on them (e.g. a Loader on `isActive`) have instantiated their content first,
    /// as Qt's synchronous property notifications do.
    started_pending: bool,
    /// Set once the agent is going away: ends its system-bus service thread.
    stop: Arc<Stop>,
}

fn locale() -> String {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .unwrap_or_else(|| "en_US.UTF-8".into())
}

fn prop(conn: &Connection, dest: &str, path: &str, iface: &str, name: &str) -> Option<OwnedValue> {
    conn.call_method(
        Some(dest),
        path,
        Some("org.freedesktop.DBus.Properties"),
        "Get",
        &(iface, name),
    )
    .ok()?
    .body()
    .deserialize::<OwnedValue>()
    .ok()
}

/// `polkit_unix_session_new_for_process(getpid())`: the logind session of this process, else
/// the graphical (display) session of its owner (systemd user services, e.g. uwsm).
fn session_id(conn: &Connection) -> Option<String> {
    let login = "org.freedesktop.login1";
    let manager = |m: &str| {
        conn.call_method(
            Some(login),
            "/org/freedesktop/login1",
            Some("org.freedesktop.login1.Manager"),
            m,
            &(std::process::id(),),
        )
        .ok()?
        .body()
        .deserialize::<OwnedObjectPath>()
        .ok()
    };
    if let Some(p) = manager("GetSessionByPID")
        && let Some(v) = prop(
            conn,
            login,
            p.as_str(),
            "org.freedesktop.login1.Session",
            "Id",
        )
        && let Ok(id) = String::try_from(v)
    {
        return Some(id);
    }
    let user = manager("GetUserByPID")?;
    let v = prop(
        conn,
        login,
        user.as_str(),
        "org.freedesktop.login1.User",
        "Display",
    )?;
    let (id, _): (String, OwnedObjectPath) = v.try_into().ok()?;
    (!id.is_empty()).then_some(id)
}

fn subject(conn: &Connection) -> Option<Subject> {
    let id = session_id(conn)?;
    let mut m = HashMap::new();
    m.insert(
        "session-id".to_string(),
        OwnedValue::try_from(ZV::from(id)).ok()?,
    );
    Some(("unix-session".into(), m))
}

/// polkitd's current connection, if it runs.
fn authority_owner(conn: &Connection) -> Option<String> {
    let name = zbus::names::BusName::try_from(AUTHORITY).ok()?;
    zbus::blocking::fdo::DBusProxy::new(conn)
        .ok()?
        .get_name_owner(name)
        .ok()
        .map(|o| o.to_string())
}

/// Starts polkitd by D-Bus activation and returns its connection, as libpolkit-agent's
/// authority proxy (`G_DBUS_PROXY_FLAGS_NONE`, polkitauthority.c) does when the name has no
/// owner: an activatable polkitd then runs, with this agent registered, before the first
/// CheckAuthorization (pkexec, udisks...) would start it and find no agent.
fn start_authority(conn: &Connection) -> Option<String> {
    let name = zbus::names::WellKnownName::try_from(AUTHORITY).ok()?;
    let r = zbus::blocking::fdo::DBusProxy::new(conn)
        .and_then(|p| p.start_service_by_name(name, 0).map_err(Into::into));
    if let Err(e) = r {
        elog::eprintln!("PolkitAgent: cannot start {AUTHORITY}: {e}");
        return None;
    }
    authority_owner(conn)
}

/// RegisterAuthenticationAgent with the polkitd connection `owner`, not whoever owns the name
/// by the time the call arrives.
fn register(conn: &Connection, owner: &str, path: &str) -> bool {
    let Some(subject) = subject(conn) else {
        elog::eprintln!(
            "PolkitAgent: failed to create subject for listener: No session for pid {}",
            std::process::id()
        );
        return false;
    };
    let r = conn.call_method(
        Some(owner),
        AUTHORITY_PATH,
        Some(AUTHORITY_IFACE),
        "RegisterAuthenticationAgent",
        &(subject, locale(), path),
    );
    if let Err(e) = &r {
        elog::eprintln!("PolkitAgent: failed to register listener on path {path}: {e}");
    }
    r.is_ok()
}

/// The agent is gone: withdraw its object, then unregister it from the polkitd it registered
/// with. The object leaves the bus first: a BeginAuthentication polkitd sends meanwhile (it
/// answers UnregisterAuthenticationAgent only after) finds no agent.
fn withdraw(conn: &Connection, path: &str, registered_with: Option<&str>) {
    let _ = conn.object_server().remove::<Agent, _>(path);
    if let Some(owner) = registered_with
        && let Some(subject) = subject(conn)
    {
        let _ = conn.call_method(
            Some(owner),
            AUTHORITY_PATH,
            Some(AUTHORITY_IFACE),
            "UnregisterAuthenticationAgent",
            &(subject, path),
        );
    }
}

/// Per agent path, the service thread of the agent started there last: polkitd keeps one agent
/// per subject and removes it by sender and path, and every agent of the process shares one
/// connection and the path, so a successor registering before its predecessor's
/// UnregisterAuthenticationAgent would be refused and then unregistered with it.
static LAST: std::sync::Mutex<std::collections::BTreeMap<String, std::sync::mpsc::Receiver<()>>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

/// Held by an agent's service thread: disconnects its successor's receiver once the thread and
/// every earlier one at the path are done with polkitd.
struct Link {
    prev: Option<std::sync::mpsc::Receiver<()>>,
    _done: std::sync::mpsc::Sender<()>,
}

impl Link {
    fn new(path: &str) -> Link {
        let (done, gone) = std::sync::mpsc::channel();
        let prev = LAST
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(path.to_string(), gone);
        Link { prev, _done: done }
    }

    /// Blocks until the predecessor's thread has ended.
    fn wait(&mut self) {
        if let Some(p) = self.prev.take() {
            // Only ever disconnected, never sent on.
            let _ = p.recv();
        }
    }
}

impl Drop for Link {
    /// A thread that never got as far as waiting still holds its successor back.
    fn drop(&mut self) {
        self.wait();
    }
}

fn parse_identity((kind, d): &(String, HashMap<String, OwnedValue>)) -> Option<Ident> {
    let num = |k: &str| d.get(k).and_then(|v| u32::try_from(v).ok());
    match kind.as_str() {
        "unix-user" => Some((false, num("uid")?)),
        "unix-group" => Some((true, num("gid")?)),
        _ => None,
    }
}

const GONE: &str = "PolkitAgent is being destroyed";

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.PolicyKit1.Error")]
enum AgentError {
    #[zbus(error)]
    ZBus(zbus::Error),
    Failed(String),
    Cancelled(String),
    NotAuthorized(String),
}

/// The `org.freedesktop.PolicyKit1.AuthenticationAgent` object polkitd calls. Served by the
/// connection's ObjectServer like every other object on the shared system bus
/// (`crate::bluez_agent`): the ObjectServer answers calls to paths it does not serve, so a raw
/// MethodCall subscription on the same connection would lose every call to `UnknownObject`.
struct Agent {
    tx: crate::svc::Sender<BusEv>,
    stop: Arc<Stop>,
}

/// Only polkitd's current connection may drive the agent.
async fn from_polkitd(conn: &zbus::Connection, hdr: &Header<'_>) -> Result<(), AgentError> {
    let owner = match zbus::fdo::DBusProxy::new(conn).await {
        Ok(d) => d
            .get_name_owner(AUTHORITY.try_into().map_err(zbus::Error::from)?)
            .await
            .ok(),
        Err(_) => None,
    };
    match (hdr.sender(), owner) {
        (Some(s), Some(o)) if s.as_str() == o.as_str() => Ok(()),
        (s, _) => {
            let who = s.map_or("an unknown sender".to_string(), |s| s.to_string());
            elog::eprintln!(
                "PolkitAgent: refused an {AGENT_IFACE} call from {who}, which is not polkitd"
            );
            Err(AgentError::NotAuthorized(
                "Only polkitd may call the authentication agent".into(),
            ))
        }
    }
}

#[zbus::interface(name = "org.freedesktop.PolicyKit1.AuthenticationAgent")]
impl Agent {
    /// Answered once the request's flow ends: success, or `Cancelled` with the reason.
    #[allow(clippy::too_many_arguments)]
    async fn begin_authentication(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        action_id: String,
        message: String,
        icon_name: String,
        _details: HashMap<String, String>,
        cookie: String,
        identities: Vec<(String, HashMap<String, OwnedValue>)>,
    ) -> Result<(), AgentError> {
        from_polkitd(conn, &hdr).await?;
        let mut seen = HashSet::new();
        let identities = identities
            .iter()
            .filter_map(parse_identity)
            .filter(|i| seen.insert(*i))
            .collect();
        let (done, answer) = async_channel::bounded(1);
        let req = Req {
            call: Caller::Polkit(done),
            action_id,
            message,
            icon_name,
            cookie,
            identities,
        };
        if self.stop.load(Ordering::Acquire) || self.tx.send(BusEv::Begin(req)).is_err() {
            return Err(AgentError::Cancelled(GONE.into()));
        }
        // A request dropped unanswered (the agent went away) ends as cancelled.
        match answer.recv().await {
            Ok(None) => Ok(()),
            Ok(Some(reason)) => Err(AgentError::Cancelled(reason)),
            Err(_) => Err(AgentError::Cancelled(GONE.into())),
        }
    }

    async fn cancel_authentication(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
        cookie: String,
    ) -> Result<(), AgentError> {
        from_polkitd(conn, &hdr).await?;
        let missing = format!("No pending authentication request for cookie '{cookie}'");
        let (done, found) = async_channel::bounded(1);
        if self.stop.load(Ordering::Acquire) || self.tx.send(BusEv::Cancel(done, cookie)).is_err() {
            return Err(AgentError::Failed(missing));
        }
        match found.recv().await {
            Ok(true) => Ok(()),
            _ => Err(AgentError::Failed(missing)),
        }
    }
}

fn owner_rule() -> Option<MatchRule<'static>> {
    Some(
        MatchRule::builder()
            .msg_type(MsgType::Signal)
            .sender("org.freedesktop.DBus")
            .ok()?
            .interface("org.freedesktop.DBus")
            .ok()?
            .member("NameOwnerChanged")
            .ok()?
            .add_arg(AUTHORITY)
            .ok()?
            .build(),
    )
}

/// Serve the agent at `path` on every system-bus connection and register it with polkitd: on
/// connect, with every new polkitd (restarted, or replacing the running one), and again on the
/// new connection once the bus itself comes back ([`crate::sysbus::spawn`] reconnects). `stop`
/// ends the thread and its subscription; the thread withdraws the agent itself, since only it
/// knows what it served and registered. It starts only after the agent's predecessor at `path`
/// has withdrawn.
fn start_bus(path: String, stop: Arc<Stop>, tx: crate::svc::Sender<BusEv>) {
    let Some(owner) = owner_rule() else { return };
    let mut link = Link::new(&path);
    let st = stop.clone();
    crate::sysbus::spawn(vec![owner], st, move |conn, owners| {
        link.wait();
        session(conn, owners, &path, &stop, &tx)
    });
}

/// One system-bus connection's life; false once the agent is gone, after withdrawing it.
fn session(
    conn: &Connection,
    owners: &Receiver<zbus::message::Message>,
    path: &str,
    stop: &Arc<Stop>,
    tx: &crate::svc::Sender<BusEv>,
) -> bool {
    let alive = || !stop.load(Ordering::Acquire);
    if !alive() {
        return false;
    }
    let agent = Agent {
        tx: tx.clone(),
        stop: stop.clone(),
    };
    match conn.object_server().at(path, agent) {
        Ok(true) => {}
        Ok(false) => {
            elog::eprintln!("PolkitAgent: cannot serve the authentication agent at {path}: taken");
            return false;
        }
        Err(e) => {
            elog::eprintln!("PolkitAgent: cannot serve the authentication agent at {path}: {e}");
            return false;
        }
    }
    // The polkitd connection last registered with (or tried). A polkitd process starts with no
    // agents, so each new owner of its name needs a registration of its own. The owner is asked
    // for, not taken from the signal: signals queued before the first answer are older than it.
    let mut polkitd: Option<String> = None;
    let mut registered = false;
    // Activated once per connection, as libpolkit-agent's proxy is made once per connection; a
    // polkitd that exits later is started again by its next client, and followed from there.
    let mut activate = true;
    let mut follow = || {
        let mut now = authority_owner(conn);
        if now.is_none() && std::mem::take(&mut activate) {
            now = start_authority(conn);
        }
        if now == polkitd {
            return true;
        }
        registered = now.as_deref().is_some_and(|o| register(conn, o, path));
        polkitd = now;
        alive() && tx.send(BusEv::Registered(registered)).is_ok()
    };
    let gone = !follow() || owners.iter().any(|_| !follow());
    if !gone && alive() {
        // The bus closed the connection, and polkitd dropped the registration with it.
        return !registered || tx.send(BusEv::Registered(false)).is_ok();
    }
    withdraw(conn, path, polkitd.as_deref().filter(|_| registered));
    false
}

/// `g_strcompress`: undo the helper's `g_strescape`.
fn unescape(s: &str) -> String {
    crate::libnotify::strcompress(s)
}

pub(crate) fn passwd_name(uid: u32) -> Option<(String, String)> {
    crate::osutil::passwd(crate::osutil::PwKey::Uid(uid), |e| {
        let s = |c: Option<&std::ffi::CStr>| {
            c.map(|c| c.to_string_lossy().into_owned())
                .filter(|s| !s.is_empty())
        };
        let name = s(e.name())?;
        let gecos = s(e.gecos()).unwrap_or_else(|| name.clone());
        Some((name, gecos))
    })
    .ok()
    .flatten()
    .flatten()
}

fn group_name(gid: u32) -> Option<String> {
    let mut buf = vec![0u8; 16384];
    // SAFETY: `group` is a C struct of integers and pointers: all-zero is a valid value.
    let mut gr: libc::group = unsafe { std::mem::zeroed() };
    let mut res: *mut libc::group = std::ptr::null_mut();
    // SAFETY: `gr`, `res` and the `buf.len()` bytes of `buf` are writable for the call.
    unsafe { libc::getgrgid_r(gid, &mut gr, buf.as_mut_ptr().cast(), buf.len(), &mut res) };
    if res.is_null() || gr.gr_name.is_null() {
        return None;
    }
    // SAFETY: getgrgid_r filled `gr`: the non-null gr_name is a NUL-terminated string in `buf`.
    let n = unsafe { std::ffi::CStr::from_ptr(gr.gr_name) }
        .to_string_lossy()
        .into_owned();
    (!n.is_empty()).then_some(n)
}

/// `polkit_agent_session_initiate`: connect to the socket-activated helper or spawn the setuid
/// one, send the cookie and stream its lines to the main loop.
fn initiate(
    id: u64,
    user: &str,
    cookie: &str,
    tx: crate::svc::Sender<(u64, SessEv)>,
) -> Option<Session> {
    let (mut input, output, child, sock): (Box<dyn Write + Send>, Box<dyn Read + Send>, _, _) =
        match std::os::unix::net::UnixStream::connect(HELPER_SOCKET) {
            Ok(s) if std::path::Path::new(HELPER_SOCKET).exists() => {
                let r = s.try_clone().ok()?;
                let sock = s.try_clone().ok()?;
                let mut w: Box<dyn Write + Send> = Box::new(s);
                w.write_all(format!("{user}\n").as_bytes()).ok()?;
                (w, Box::new(r), None, Some(sock))
            }
            _ => {
                let helper = HELPERS
                    .iter()
                    .find(|p| std::path::Path::new(p).exists())
                    .or_else(|| {
                        elog::eprintln!(
                            "PolkitAgent: Cannot spawn helper: polkit-agent-helper-1 not found"
                        );
                        None
                    })?;
                // This process's environment, the config's pragmas applied, not the user's
                // (`workpool::userenv`): in Quickshell libpolkit-agent starts the helper
                // inside the shell's process, past `setupProcessEnvironment`.
                let mut c = std::process::Command::new(helper)
                    .arg(user)
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .spawn()
                    .map_err(|e| elog::eprintln!("PolkitAgent: Cannot spawn helper: {e}"))
                    .ok()?;
                let (i, o) = (c.stdin.take()?, c.stdout.take()?);
                let child = ChildRef::new(c.id() as libc::pid_t);
                if child.is_err() {
                    let _ = c.kill();
                }
                workpool::reap(c);
                let child = child
                    .map_err(|e| elog::eprintln!("PolkitAgent: helper: pidfd_open: {e}"))
                    .ok()?;
                (Box::new(i), Box::new(o), Some(child), None)
            }
        };
    input.write_all(format!("{cookie}\n").as_bytes()).ok()?;
    let read = move || {
        let mut r = BufReader::new(output);
        let mut line = String::new();
        loop {
            line.clear();
            match r.read_line(&mut line) {
                Ok(n) if n > 0 => {
                    let l = line.strip_suffix('\n').unwrap_or(&line);
                    if tx.send((id, SessEv::Line(unescape(l)))).is_err() {
                        return;
                    }
                }
                _ => break,
            }
        }
        let _ = tx.send((id, SessEv::End));
    };
    let reader = std::thread::Builder::new()
        .name("polkit-helper".into())
        .spawn(read);
    let session = Session {
        id,
        input,
        child,
        sock,
        asked: false,
    };
    if let Err(e) = reader {
        // The system refused the thread (`EAGAIN`): nothing reads the helper's lines.
        elog::eprintln!("PolkitAgent: Cannot read the helper: {e}");
        session.kill();
        return None;
    }
    Some(session)
}

fn reply(call: &Caller, cancel: Option<&str>) {
    match call {
        Caller::Polkit(done) => {
            let _ = done.try_send(cancel.map(str::to_string));
        }
        // A typed key was answered by `submit()`; a refusal tells bluetoothd why.
        Caller::Bluez(e) => {
            if let Some(reason) = cancel {
                e.answer(Reply::Rejected(reason.into()));
            }
        }
    }
}

impl Polkit {
    fn start(w: &mut World, agent: usize) -> Polkit {
        let (p, tx) = Polkit::new(w, agent);
        // Subscribing and registering wait on the system bus: the service thread reports the
        // registration through `bus`.
        start_bus(p.path.clone(), p.stop.clone(), tx);
        p
    }

    /// The agent's state with nothing on the system bus yet, and the sender its service thread
    /// reports through.
    fn new(w: &mut World, agent: usize) -> (Polkit, crate::svc::Sender<BusEv>) {
        let mut path = match w.read(agent, "path") {
            Some(Value::Str(s)) => s.to_string(),
            _ => String::new(),
        };
        if path.is_empty() {
            path = DEFAULT_PATH.into();
            w.write(agent, "path", Value::str(&path));
        }
        let stop = Arc::new(Stop::default());
        let h = w.waker.handle(crate::svc::Svc::Polkit);
        let (tx, bus) = h.channel();
        let (sess_tx, sess_rx) = h.channel();
        let (bt_tx, bt) = h.channel();
        let p = Polkit {
            bt,
            _dialog: crate::bluez_agent::attach_dialog(bt_tx),
            agent,
            path,
            bus,
            registered: false,
            queue: VecDeque::new(),
            flow: None,
            sess_tx,
            sess_rx,
            next_session: 0,
            doomed: vec![],
            started_pending: false,
            stop,
        };
        (p, tx)
    }

    /// A BlueZ PIN/passkey entry, queued like a polkit request for the current user.
    fn begin_bluez(&mut self, w: &mut World, e: Entry) {
        let (message, action) = if e.passkey {
            (
                format!("Enter the passkey shown on {}", e.alias),
                "org.bluez.Agent1.RequestPasskey",
            )
        } else {
            (
                format!("Enter the PIN shown on or provided with {}", e.alias),
                "org.bluez.Agent1.RequestPinCode",
            )
        };
        let req = Req {
            call: Caller::Bluez(e),
            action_id: action.into(),
            message,
            icon_name: "bluetooth".into(),
            cookie: String::new(),
            // SAFETY: getuid(2) takes no arguments and always succeeds.
            identities: vec![(false, unsafe { libc::getuid() })],
        };
        self.begin(w, req);
    }

    /// bluetoothd cancelled entry `id`: close its flow (next request starts) or drop it queued.
    fn cancel_bluez(&mut self, w: &mut World, id: u64) {
        let ours = |r: &Req| matches!(&r.call, Caller::Bluez(e) if e.id == id);
        if let Some(fl) = self.flow.as_mut().filter(|f| ours(&f.req)) {
            if let Caller::Bluez(e) = &fl.req.call {
                e.answer(Reply::Canceled);
            }
            fl.cancelled = true;
            let f = fl.item;
            w.write(f, "isCancelled", Value::Bool(true));
            self.completed(w, false, Some("Pairing request was cancelled."));
            crate::native::emit(w, f, "authenticationRequestCancelled", &[], vec![]);
        } else if let Some(k) = self.queue.iter().position(ours) {
            let r = self.queue.remove(k).unwrap();
            if let Caller::Bluez(e) = &r.call {
                e.answer(Reply::Canceled);
            }
        }
    }

    fn begin(&mut self, w: &mut World, req: Req) {
        self.queue.push_back(req);
        if self.flow.is_none() {
            self.activate(w);
        }
    }

    /// activateAuthenticationRequest(): start the next queued request.
    fn activate(&mut self, w: &mut World) {
        while self.flow.is_none() {
            let Some(req) = self.queue.pop_front() else {
                return;
            };
            let mut ids = vec![];
            for &(group, id) in &req.identities {
                let (name, display) = if group {
                    let n = group_name(id).unwrap_or_else(|| id.to_string());
                    (n.clone(), n)
                } else {
                    passwd_name(id).unwrap_or_else(|| (id.to_string(), id.to_string()))
                };
                ids.push((group, id, name, display));
            }
            if ids.is_empty() {
                elog::eprintln!(
                    "PolkitAgent: no supported identities available for authentication request, cancelling."
                );
                reply(
                    &req.call,
                    Some("Error requesting authentication: no supported identities available."),
                );
                continue;
            }
            let f = crate::types::native_singleton(w, "AuthFlow");
            let mut identities = vec![];
            for (group, id, name, display) in ids {
                let i = w.new_inst("Identity", Some(f));
                for (k, v) in [
                    ("id", Value::Num(id as f64)),
                    ("string", Value::str(&name)),
                    ("displayName", Value::str(&display)),
                    ("isGroup", Value::Bool(group)),
                ] {
                    w.declare(i, k, "var");
                    w.write(i, k, v);
                }
                identities.push(Identity {
                    item: i,
                    ident: (group, id),
                    name,
                });
            }
            w.write(f, "message", Value::str(&req.message));
            w.write(f, "iconName", Value::str(&req.icon_name));
            w.write(f, "actionId", Value::str(&req.action_id));
            w.write(f, "cookie", Value::str(&req.cookie));
            w.write(
                f,
                "identities",
                Value::array(identities.iter().map(|i| Value::Item(i.item)).collect()),
            );
            w.write(f, "selectedIdentity", Value::Item(identities[0].item));
            self.flow = Some(Flow {
                item: f,
                req,
                identities,
                selected: 0,
                session: None,
                cancelled: false,
            });
            self.setup_session(w);
            if let Some(fl) = &self.flow {
                let f = fl.item;
                w.write(self.agent, "flow", Value::Item(f));
                w.write(self.agent, "isActive", Value::Bool(true));
                self.started_pending = true;
                // announce() emits authenticationRequestStarted after the settle that follows,
                // in a poll: a request started from QML (the one before it was dismissed) needs
                // one, and a BlueZ flow has no helper whose output would wake the service.
                self.sess_tx.handle().wake();
            }
        }
    }

    /// AuthFlow::setupSession(): replace the helper session for the selected identity.
    fn setup_session(&mut self, w: &mut World) {
        let Some(fl) = self.flow.as_mut() else { return };
        if let Some(s) = fl.session.take() {
            s.kill();
        }
        self.next_session += 1;
        let id = self.next_session;
        if let Caller::Bluez(e) = &fl.req.call {
            let prompt = if e.passkey { "Passkey" } else { "PIN" };
            w.write(fl.item, "isResponseRequired", Value::Bool(true));
            w.write(fl.item, "inputPrompt", Value::str(prompt));
            w.write(fl.item, "responseVisible", Value::Bool(true));
            return;
        }
        let ident = &fl.identities[fl.selected];
        let s = if ident.ident.0 {
            elog::eprintln!("PolkitAgent: Unsupported identity type");
            None
        } else {
            initiate(id, &ident.name, &fl.req.cookie, self.sess_tx.clone())
        };
        match s {
            Some(s) => fl.session = Some(s),
            // complete_session(FALSE) without a helper. quickshell would restart the session
            // forever; report the failure once and wait for the user instead.
            None => {
                w.write(fl.item, "failed", Value::Bool(true));
                crate::native::emit(w, fl.item, "authenticationFailed", &[], vec![]);
            }
        }
    }

    /// AuthFlow::completed(gainedAuthorization); `cancel` = reason for a cancelled request.
    fn completed(&mut self, w: &mut World, gained: bool, cancel: Option<&str>) {
        let Some(fl) = self.flow.as_mut() else { return };
        let asked = fl.session.take().map(|s| {
            let asked = s.asked;
            s.kill();
            asked
        });
        let f = fl.item;
        if gained {
            w.write(f, "isCompleted", Value::Bool(true));
            w.write(f, "isSuccessful", Value::Bool(true));
            self.finish(w, None);
            crate::native::emit(w, f, "authenticationSucceeded", &[], vec![]);
        } else if fl.cancelled {
            w.write(f, "isCompleted", Value::Bool(true));
            w.write(f, "isSuccessful", Value::Bool(false));
            self.finish(
                w,
                Some(cancel.unwrap_or("Authentication request was cancelled")),
            );
        } else {
            w.write(f, "failed", Value::Bool(true));
            crate::native::emit(w, f, "authenticationFailed", &[], vec![]);
            // A helper that ended without asking anything (PAM refused before any prompt, as
            // pam_faillock does for a locked account) would end the same way again at once:
            // quickshell restarts it forever. Report the failure once and wait for the user, as
            // without a helper.
            if asked != Some(false) {
                self.setup_session(w);
            }
        }
    }

    /// finishAuthenticationRequest(): answer polkitd, drop the flow, start the next request.
    fn finish(&mut self, w: &mut World, cancel: Option<&str>) {
        let Some(fl) = self.flow.take() else { return };
        reply(&fl.req.call, cancel);
        self.doomed.push(fl.item);
        w.write(self.agent, "flow", Value::Null);
        w.write(self.agent, "isActive", Value::Bool(false));
        self.activate(w);
    }

    fn cancel_user(&mut self, w: &mut World) {
        let Some(fl) = self.flow.as_mut() else { return };
        if fl.session.is_none() && fl.cancelled {
            return;
        }
        fl.cancelled = true;
        w.write(fl.item, "isCancelled", Value::Bool(true));
        self.completed(w, false, Some("Authentication request cancelled by user."));
    }

    fn cancel_agent(&mut self, w: &mut World, done: async_channel::Sender<bool>, cookie: &str) {
        let found = if let Some(fl) = self.flow.as_mut().filter(|f| f.req.cookie == cookie) {
            fl.cancelled = true;
            let f = fl.item;
            w.write(f, "isCancelled", Value::Bool(true));
            self.completed(w, false, Some("Authentication request cancelled by agent."));
            crate::native::emit(w, f, "authenticationRequestCancelled", &[], vec![]);
            true
        } else if let Some(k) = self.queue.iter().position(|r| r.cookie == cookie) {
            let r = self.queue.remove(k).unwrap();
            reply(&r.call, Some("Authentication request was cancelled"));
            true
        } else {
            false
        };
        let _ = done.try_send(found);
    }

    fn submit(&mut self, w: &mut World, value: String) {
        let Some(fl) = self.flow.as_mut() else {
            zero_string(value);
            return;
        };
        if let Caller::Bluez(e) = &fl.req.call {
            let key = value.trim();
            let (ok, error) = if e.passkey {
                (
                    crate::bluez_agent::parse_passkey(key).map(|k| k.to_string()),
                    "The passkey is a number from 0 to 999999",
                )
            } else {
                (
                    crate::bluez_agent::valid_pin(key).then(|| key.to_string()),
                    "The PIN is 1 to 16 characters long",
                )
            };
            zero_string(value);
            let f = fl.item;
            match ok {
                Some(k) => {
                    e.answer(Reply::Key(k));
                    w.write(f, "isResponseRequired", Value::Bool(false));
                    self.completed(w, true, None);
                }
                None => {
                    w.write(f, "supplementaryMessage", Value::str(error));
                    w.write(f, "supplementaryIsError", Value::Bool(true));
                    w.write(f, "failed", Value::Bool(true));
                    crate::native::emit(w, f, "authenticationFailed", &[], vec![]);
                }
            }
            return;
        }
        let Some(s) = fl.session.as_mut() else {
            zero_string(value);
            return;
        };
        let mut b = value.into_bytes();
        if b.last() != Some(&b'\n') {
            b.push(b'\n');
        }
        let _ = s.input.write_all(&b).and_then(|_| s.input.flush());
        zero(&mut b);
        w.write(fl.item, "isResponseRequired", Value::Bool(false));
        w.write(fl.item, "inputPrompt", Value::str(""));
        w.write(fl.item, "responseVisible", Value::Bool(false));
    }

    fn line(&mut self, w: &mut World, l: &str) {
        let Some(fl) = self.flow.as_mut() else { return };
        if l.starts_with("PAM_PROMPT_ECHO_")
            && let Some(s) = fl.session.as_mut()
        {
            s.asked = true;
        }
        let f = fl.item;
        let request = |w: &mut World, msg: &str, echo: bool| {
            w.write(f, "isResponseRequired", Value::Bool(true));
            w.write(f, "inputPrompt", Value::str(msg));
            w.write(f, "responseVisible", Value::Bool(echo));
        };
        let info = |w: &mut World, msg: &str, err: bool| {
            w.write(f, "supplementaryMessage", Value::str(msg));
            w.write(f, "supplementaryIsError", Value::Bool(err));
        };
        if let Some(m) = l.strip_prefix("PAM_PROMPT_ECHO_OFF ") {
            request(w, m, false);
        } else if let Some(m) = l.strip_prefix("PAM_PROMPT_ECHO_ON ") {
            request(w, m, true);
        } else if let Some(m) = l.strip_prefix("PAM_ERROR_MSG ") {
            info(w, m, true);
        } else if let Some(m) = l.strip_prefix("PAM_TEXT_INFO ") {
            info(w, m, false);
        } else if l.starts_with("SUCCESS") {
            self.completed(w, true, None);
        } else if l.starts_with("FAILURE") {
            self.completed(w, false, None);
        } else {
            elog::eprintln!("PolkitAgent: Unknown line from helper");
        }
    }

    /// Emits the `authenticationRequestStarted` a started request still waits for (see
    /// `started_pending`); returns whether it did. The request's own events come after it, as
    /// in quickshell, which emits it while starting the request: a helper that answers at once
    /// can have its lines queued by the time the loop looks again.
    fn announce(&mut self, w: &mut World) -> bool {
        let started = std::mem::take(&mut self.started_pending) && self.flow.is_some();
        if started {
            crate::native::emit(w, self.agent, "authenticationRequestStarted", &[], vec![]);
        }
        started
    }

    /// QML wrote `flow.selectedIdentity` / `agent.path`: apply like the C++ setters.
    /// Returns whether `authenticationRequestStarted` was emitted.
    fn sync(&mut self, w: &mut World) -> bool {
        let started = self.announce(w);
        let path = match w.read(self.agent, "path") {
            Some(Value::Str(s)) => s.to_string(),
            _ => String::new(),
        };
        if path != self.path {
            elog::eprintln!("PolkitAgent: cannot change path after it has been set.");
            w.write(self.agent, "path", Value::str(&self.path));
        }
        let Some(fl) = self.flow.as_mut() else {
            return started;
        };
        let cur = w.read(fl.item, "selectedIdentity");
        let want = fl.identities[fl.selected].item;
        if matches!(cur, Some(Value::Item(i)) if i == want) {
            return started;
        }
        match cur {
            Some(Value::Item(i)) if fl.identities.iter().any(|x| x.item == i) => {
                fl.selected = fl.identities.iter().position(|x| x.item == i).unwrap();
                // Session::cancel() reports a failed attempt before the new session starts.
                if let Some(s) = fl.session.take() {
                    s.kill();
                }
                w.write(fl.item, "failed", Value::Bool(true));
                crate::native::emit(w, fl.item, "authenticationFailed", &[], vec![]);
                self.setup_session(w);
            }
            _ => {
                elog::eprintln!("AuthFlow: Cannot set selected identity to null.");
                w.write(fl.item, "selectedIdentity", Value::Item(want));
            }
        }
        started
    }

    /// cancelAllRequests(): the agent is going away.
    fn shutdown(&mut self, w: &mut World) {
        self.stop.store(true, Ordering::SeqCst);
        for r in std::mem::take(&mut self.queue).into_iter().rev() {
            reply(&r.call, Some(GONE));
        }
        self.cancel_user(w);
    }
}

impl Drop for Polkit {
    /// The World went away with the agent: its service thread withdraws it.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn zero_string(s: String) {
    let mut b = s.into_bytes();
    zero(&mut b);
}

/// `AuthFlow.submit()` / `cancelAuthenticationRequest()` from QML.
pub(crate) fn flow_call(w: &mut World, flow: usize, name: &str, args: &[Value]) {
    let Some(mut p) = w.polkit.take() else { return };
    if p.flow.as_ref().is_some_and(|f| f.item == flow) {
        if name == "submit" {
            let v = match args.first() {
                Some(Value::Str(s)) => s.to_string(),
                Some(v) => v.to_js_string(),
                None => String::new(),
            };
            p.submit(w, v);
        } else {
            p.cancel_user(w);
        }
    }
    w.polkit = Some(p);
}

impl Runtime {
    /// Drive the (single) registered `PolkitAgent`; returns whether anything changed. Events
    /// are applied one per settle so handlers observe each state like Qt's queued signals.
    pub(crate) fn poll_polkit(&mut self) -> bool {
        let (rev, started) = {
            let mut w = self.world.borrow_mut();
            let agents = w.of_type("PolkitAgent");
            if agents.is_empty() && w.polkit.is_none() {
                return false;
            }
            let rev = w.revision;
            let mut p = match w.polkit.take() {
                Some(mut p) if !agents.contains(&p.agent) => {
                    p.shutdown(&mut w);
                    for f in std::mem::take(&mut p.doomed) {
                        w.destroy(f);
                    }
                    None
                }
                p => p,
            };
            if p.is_none()
                && let Some(&a) = agents.first()
            {
                p = Some(Polkit::start(&mut w, a));
            }
            if let Some(mut p) = p {
                for f in std::mem::take(&mut p.doomed) {
                    w.destroy(f);
                }
                w.mirror(p.agent, crate::svc::Svc::Polkit);
                if let Some(fl) = &p.flow {
                    w.mirror(fl.item, crate::svc::Svc::Polkit);
                }
                let started = p.sync(&mut w);
                w.polkit = Some(p);
                (rev, started)
            } else {
                (rev, false)
            }
        };
        // Deliver the signal now: a BlueZ flow has no helper whose output would settle later.
        let mut changed = started;
        if started {
            self.settle();
        }
        loop {
            let mut w = self.world.borrow_mut();
            let Some(mut p) = w.polkit.take() else { break };
            // The request the last event started, before any event of its own.
            let handled = if p.announce(&mut w) {
                true
            } else if let Ok(ev) = p.bus.try_recv() {
                match ev {
                    BusEv::Registered(ok) => {
                        if ok != p.registered {
                            w.write(p.agent, "isRegistered", Value::Bool(ok));
                        }
                        p.registered = ok;
                    }
                    BusEv::Begin(req) => p.begin(&mut w, req),
                    BusEv::Cancel(done, cookie) => p.cancel_agent(&mut w, done, &cookie),
                }
                true
            } else if let Ok(t) = p.bt.try_recv() {
                match t {
                    Typed::Ask(e) => p.begin_bluez(&mut w, e),
                    Typed::Cancel(id) => p.cancel_bluez(&mut w, id),
                }
                true
            } else if let Ok((id, ev)) = p.sess_rx.try_recv() {
                let current = p
                    .flow
                    .as_ref()
                    .and_then(|f| f.session.as_ref())
                    .is_some_and(|s| s.id == id);
                if current {
                    match ev {
                        SessEv::Line(l) => p.line(&mut w, &l),
                        SessEv::End => p.completed(&mut w, false, None),
                    }
                }
                true
            } else {
                false
            };
            w.polkit = Some(p);
            drop(w);
            if !handled {
                break;
            }
            changed = true;
            self.settle();
        }
        changed || self.world.borrow().revision != rev
    }
}

#[cfg(test)]
mod tests {
    use super::{BusEv, Caller, Polkit, Req, SessEv};

    /// A helper that answers at once (PAM refuses a locked account before any prompt) can have
    /// its lines queued before the loop that took the request looks again. A window shown by
    /// `authenticationRequestStarted` must still hear of the request before its failure, and
    /// hear of it at all when the failure makes QML dismiss the request.
    #[test]
    fn a_request_is_announced_before_its_helpers_answer() {
        const TEST: &str = concat!(
            module_path!(),
            "::a_request_is_announced_before_its_helpers_answer"
        );
        if !crate::testenv::in_own_process(TEST) {
            crate::testenv::run_in_own_process_with_own_run(TEST, [("LC_ALL", "C")]);
            return;
        }
        // The helper the session connects to; its answer is queued by hand below.
        std::fs::create_dir_all("/run/polkit").unwrap();
        let _helper = std::os::unix::net::UnixListener::bind(super::HELPER_SOCKET).unwrap();
        let mut rt = crate::runtime::Runtime::new(None);
        let r = rt
            .load_str(
                "import QtQuick\nimport Rustshell.Services.Polkit\nItem { id: r\n\
                 property string log: \"\"\n\
                 PolkitAgent { id: agent\n\
                   onAuthenticationRequestStarted: r.log += \"started;\"\n\
                   property bool failed: flow ? flow.failed : false\n\
                   onFailedChanged: if (failed) { r.log += \"failed;\"; flow.cancelAuthenticationRequest() } } }",
            )
            .unwrap();
        rt.settle();
        let (bus, sess) = {
            let mut w = rt.world.borrow_mut();
            let agent = w.of_type("PolkitAgent")[0];
            let (p, bus) = Polkit::new(&mut w, agent);
            let sess = p.sess_tx.clone();
            w.polkit = Some(p);
            (bus, sess)
        };
        let (done, reply) = async_channel::bounded(1);
        let req = Req {
            call: Caller::Polkit(done),
            action_id: "org.example.act".into(),
            message: "Authenticate".into(),
            icon_name: String::new(),
            cookie: "c1".into(),
            identities: vec![(false, 0)],
        };
        bus.send(BusEv::Begin(req)).ok().unwrap();
        // The request's session is the agent's first.
        for l in ["PAM_ERROR_MSG The account is locked", "FAILURE"] {
            sess.send((1, SessEv::Line(l.into()))).ok().unwrap();
        }
        rt.poll_native();
        rt.settle();
        assert_eq!(rt.get(r, "log").to_js_string(), "started;failed;");
        assert_eq!(
            reply.try_recv().ok(),
            Some(Some("Authentication request cancelled by user.".into()))
        );
    }

    #[test]
    fn helper_lines_unescape_like_g_strcompress() {
        assert_eq!(super::unescape(r"a\tb\\c\n"), "a\tb\\c\n");
        // g_strescape octal-escapes non-ASCII bytes: "Пароль: " from a ru_RU helper.
        assert_eq!(
            super::unescape(r"\320\237\320\260\321\200\320\276\320\273\321\214: "),
            "Пароль: "
        );
        assert_eq!(super::unescape(r#"say \"hi\""#), "say \"hi\"");
    }

    /// The system refuses the thread that reads the helper's lines (the limit on threads is
    /// reached): the session fails to start, which the agent reports as a failed
    /// authentication, instead of a panic of the shell, and the helper's connection is
    /// closed, so the helper does not wait for a response nobody will send. The helper here
    /// reads up to that close. The refusal is in the log (the test reads what its own process
    /// wrote to its stderr).
    #[test]
    fn a_refused_reader_thread_fails_the_session() {
        use std::io::Read;
        const TEST: &str = concat!(
            module_path!(),
            "::a_refused_reader_thread_fails_the_session"
        );
        if !crate::testenv::in_own_process(TEST) {
            crate::testenv::run_in_own_process_with_own_run(TEST, [("LC_ALL", "C")]);
            return;
        }
        std::fs::create_dir_all("/run/polkit").unwrap();
        let helper = std::os::unix::net::UnixListener::bind(super::HELPER_SOCKET).unwrap();
        let waker = crate::svc::Waker::default();
        let (tx, _rx) = waker.handle(crate::svc::Svc::Polkit).channel();
        crate::testenv::refuse_new_threads();
        let (session, log) =
            crate::testenv::stderr_with(|| super::initiate(1, "user", "cookie", tx));
        assert!(session.is_none());
        assert!(
            log.contains("PolkitAgent: Cannot read the helper:"),
            "stderr: {log:?}"
        );
        let (mut conn, _) = helper.accept().unwrap();
        let mut said = String::new();
        conn.read_to_string(&mut said).unwrap();
        assert_eq!(said, "user\ncookie\n");
    }
}
