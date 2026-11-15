//! BlueZ pairing agent (org.bluez.Agent1), registered as the default agent while bluetoothd runs.
//! Quickshell has none, so pairing from the original shell needs a separate agent (blueman,
//! bluedevil); without one bluetoothd fails pairing with "No agent available". impulse-shell deviates
//! here on purpose: questions and passkeys go to the user as notifications with actions, and a
//! PIN/passkey to type goes to the shell's authentication dialog: the native `PolkitAgent`
//! (`crate::polkit`) queues it as a flow next to polkit requests, so the original's polkit window asks.
//! The agent offers `KeyboardDisplay` only while such a dialog exists, else `DisplayYesNo`, so
//! bluetoothd never picks passkey entry when nothing could take it.

use crate::svc::Stop;
use crate::sysbus;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use zbus::blocking::Connection;
use zbus::message::{Header, Message};
use zbus::zvariant::{ObjectPath, OwnedValue, Value};

pub(crate) const PATH: &str = "/org/rustshell/BluetoothAgent";
/// Can show a passkey and take yes/no; typing one needs the dialog (see [`attach_dialog`]).
const DISPLAY_YES_NO: &str = "DisplayYesNo";
const KEYBOARD_DISPLAY: &str = "KeyboardDisplay";
const BLUEZ: &str = "org.bluez";
const MANAGER: &str = "org.bluez.AgentManager1";
const DEVICE: &str = "org.bluez.Device1";
const NOTIFY: &str = "org.freedesktop.Notifications";
const NOTIFY_PATH: &str = "/org/freedesktop/Notifications";

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.bluez.Error")]
enum Error {
    #[zbus(error)]
    ZBus(zbus::Error),
    Rejected(String),
    Canceled(String),
}

enum Answer {
    Key(String),
    Closed,
    Canceled,
}

#[derive(Default)]
struct Agent {
    /// Cancel flags of the requests in flight ([`Agent::open`]).
    open: Mutex<Vec<Arc<Stop>>>,
    /// The notification showing a passkey/PIN to type (0: none), replaced while typing goes
    /// on. Used on [`DISPLAY`] only.
    shown: Arc<Mutex<u32>>,
}

/// A request from bluetoothd, open until its method returns. zbus runs every method call as a
/// task of its own, so Agent1.Cancel can come while the request awaits anything (the sender
/// check, the device's alias): the request is registered before its first await and checks
/// `stop` after each one.
struct Open<'a> {
    agent: &'a Agent,
    stop: Arc<Stop>,
}

impl Open<'_> {
    /// `Canceled` once bluetoothd cancelled the request.
    fn live(&self) -> Result<(), Error> {
        if self.stop.load(Ordering::Acquire) {
            Err(Error::Canceled("Canceled".into()))
        } else {
            Ok(())
        }
    }
}

impl Drop for Open<'_> {
    fn drop(&mut self) {
        self.agent
            .open
            .lock()
            .unwrap()
            .retain(|c| !Arc::ptr_eq(c, &self.stop));
    }
}

/// Shows and closes the passkey/PIN notification in the order the agent's calls asked for it.
static DISPLAY: workpool::Lane = workpool::Lane::new();

/// What the dialog sends back for an [`Entry`].
pub(crate) enum Reply {
    Key(String),
    Rejected(String),
    Canceled,
}

/// bluetoothd asking for a PIN code (`passkey` false) or a passkey to be typed.
pub(crate) struct Entry {
    pub id: u64,
    pub passkey: bool,
    pub alias: String,
    reply: async_channel::Sender<Reply>,
}

impl Entry {
    /// Answer once; later answers are dropped.
    pub fn answer(&self, r: Reply) {
        let _ = self.reply.try_send(r);
    }
}

/// Agent → dialog.
pub(crate) enum Typed {
    Ask(Entry),
    /// bluetoothd cancelled entry `id` (Agent1.Cancel, or the deadline ran out).
    Cancel(u64),
}

struct Dialog {
    token: u64,
    tx: crate::svc::Sender<Typed>,
}

static DIALOG: Mutex<Option<Dialog>> = Mutex::new(None);
static NEXT: AtomicU64 = AtomicU64::new(1);

struct Registered {
    conn: Connection,
    capability: &'static str,
}

#[derive(Default)]
struct State {
    /// The connection serving the agent object.
    served: Option<Connection>,
    /// The registration with the running bluetoothd, if any.
    registered: Option<Registered>,
}

/// Serving, registering and withdrawing the agent, one at a time.
static STATE: Mutex<State> = Mutex::new(State {
    served: None,
    registered: None,
});

/// Live Bluetooth services (one per World); the agent is withdrawn once none is left.
static HOLDERS: AtomicUsize = AtomicUsize::new(0);

/// A Bluetooth service's claim on the agent: the last one dropped withdraws it from bluetoothd
/// and from the bus, as a pairing agent whose shell is gone must not answer for it.
pub(crate) struct Hold(());

pub(crate) fn hold() -> Hold {
    HOLDERS.fetch_add(1, Ordering::SeqCst);
    Hold(())
}

impl Drop for Hold {
    fn drop(&mut self) {
        if HOLDERS.fetch_sub(1, Ordering::SeqCst) == 1 {
            // Bus calls, off the main thread, queued with the other calls to bluetoothd.
            crate::bluez::CALLS.run(withdraw);
        }
    }
}

/// The object leaves the bus before bluetoothd hears of it (as for polkit's agent): a request
/// bluetoothd sends meanwhile finds no agent. Questions still open end now.
fn withdraw() {
    let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
    // A new service started meanwhile keeps it.
    if HOLDERS.load(Ordering::SeqCst) > 0 {
        return;
    }
    if let Some(conn) = st.served.take() {
        let os = conn.object_server();
        if let Ok(agent) = os.interface::<_, Agent>(PATH) {
            agent.get().cancel_all();
        }
        let _ = os.remove::<Agent, _>(PATH);
    }
    if let Some(r) = st.registered.take() {
        match manager(&r.conn, "UnregisterAgent", None) {
            Ok(_) => elog::eprintln!("Bluetooth: pairing agent unregistered"),
            Err(e) => elog::eprintln!("Bluetooth: cannot unregister the pairing agent: {e}"),
        }
    }
}

/// Keeps the dialog attached; dropping it detaches (the agent stops offering a keyboard).
pub(crate) struct DialogGuard(u64);

impl Drop for DialogGuard {
    fn drop(&mut self) {
        let mut d = DIALOG.lock().unwrap_or_else(|e| e.into_inner());
        if d.as_ref().is_some_and(|d| d.token == self.0) {
            *d = None;
            drop(d);
            reregister();
        }
    }
}

/// Route PIN/passkey entry to `tx` (the PolkitAgent's queue) until the guard is dropped.
pub(crate) fn attach_dialog(tx: crate::svc::Sender<Typed>) -> DialogGuard {
    let token = NEXT.fetch_add(1, Ordering::Relaxed);
    *DIALOG.lock().unwrap_or_else(|e| e.into_inner()) = Some(Dialog { token, tx });
    reregister();
    DialogGuard(token)
}

fn capability() -> &'static str {
    if DIALOG.lock().unwrap_or_else(|e| e.into_inner()).is_some() {
        KEYBOARD_DISPLAY
    } else {
        DISPLAY_YES_NO
    }
}

/// Hand `t` to the dialog; false when there is none (a dead one is detached).
fn to_dialog(t: Typed) -> bool {
    let mut d = DIALOG.lock().unwrap_or_else(|e| e.into_inner());
    let Some(dialog) = d.as_ref() else {
        return false;
    };
    if dialog.tx.send(t).is_ok() {
        return true;
    }
    *d = None;
    drop(d);
    reregister();
    false
}

fn manager(conn: &Connection, method: &str, cap: Option<&str>) -> zbus::Result<Message> {
    let path = ObjectPath::from_static_str_unchecked(PATH);
    match cap {
        Some(c) => sysbus::call(conn, BLUEZ, "/org/bluez", MANAGER, method, &(&path, c)),
        None => sysbus::call(conn, BLUEZ, "/org/bluez", MANAGER, method, &(&path,)),
    }
}

/// Register with `cap` (replacing a registration with another capability) and become the
/// default agent. Blocking.
fn apply(conn: &Connection, cap: &'static str, reg: &mut Option<Registered>) {
    let mut r = manager(conn, "RegisterAgent", Some(cap)).map(drop);
    if let Err(zbus::Error::MethodError(n, _, _)) = &r
        && n.as_str() == "org.bluez.Error.AlreadyExists"
    {
        r = if reg.as_ref().is_some_and(|x| x.capability == cap) {
            Ok(())
        } else {
            let _ = manager(conn, "UnregisterAgent", None);
            manager(conn, "RegisterAgent", Some(cap)).map(drop)
        };
    }
    if let Err(e) = r {
        elog::eprintln!("Bluetooth: cannot register the pairing agent: {e}");
        *reg = None;
        return;
    }
    *reg = Some(Registered {
        conn: conn.clone(),
        capability: cap,
    });
    match manager(conn, "RequestDefaultAgent", None) {
        Ok(_) => elog::eprintln!("Bluetooth: pairing agent registered ({cap})"),
        Err(e) => elog::eprintln!("Bluetooth: cannot make the pairing agent the default: {e}"),
    }
}

/// The dialog came or went: register again with the matching capability (off this thread).
/// Where the system refuses the thread the agent keeps the capability it is registered with
/// until the dialog next comes or goes.
fn reregister() {
    let register_again = || {
        let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
        let cap = capability();
        let Some(conn) = st
            .registered
            .as_ref()
            .filter(|r| r.capability != cap)
            .map(|r| r.conn.clone())
        else {
            return;
        };
        apply(&conn, cap, &mut st.registered);
    };
    if let Err(e) = std::thread::Builder::new()
        .name("bluez-agent".into())
        .spawn(register_again)
    {
        elog::eprintln!("Bluetooth: cannot start a thread to register the pairing agent: {e}");
    }
}

/// Register the agent with bluetoothd on `conn` (blocking; not on a zbus executor thread).
pub(crate) fn register(conn: &Connection) {
    let owned = zbus::blocking::fdo::DBusProxy::new(conn).and_then(|d| {
        d.name_has_owner(BLUEZ.try_into().map_err(zbus::Error::from)?)
            .map_err(Into::into)
    });
    if !matches!(owned, Ok(true)) {
        return;
    }
    let mut st = STATE.lock().unwrap_or_else(|e| e.into_inner());
    // Every Bluetooth service is gone: its withdrawal runs (or ran) after this.
    if HOLDERS.load(Ordering::SeqCst) == 0 {
        return;
    }
    if let Err(e) = conn.object_server().at(PATH, Agent::default()) {
        elog::eprintln!("Bluetooth: cannot serve the pairing agent: {e}");
        return;
    }
    st.served = Some(conn.clone());
    apply(conn, capability(), &mut st.registered);
}

/// bluetoothd left the bus: no Agent1.Cancel will come for the requests it made, and no one
/// takes their answers, so they end now.
pub(crate) fn bluez_gone(conn: &Connection) {
    if let Ok(agent) = conn.object_server().interface::<_, Agent>(PATH) {
        agent.get().cancel_all();
    }
}

fn hints(urgency: u8) -> HashMap<&'static str, Value<'static>> {
    HashMap::from([("urgency", Value::U8(urgency))])
}

/// Show a notification on the session bus; its id.
fn notify(
    conn: &Connection,
    replaces: u32,
    body: &str,
    actions: &[&str],
    urgency: u8,
    expire: i32,
) -> Option<u32> {
    let r = sysbus::call(
        conn,
        NOTIFY,
        NOTIFY_PATH,
        NOTIFY,
        "Notify",
        &(
            "Bluetooth",
            replaces,
            "bluetooth",
            "Bluetooth pairing",
            body,
            actions,
            hints(urgency),
            expire,
        ),
    );
    match r.and_then(|m| m.body().deserialize::<(u32,)>()) {
        Ok((id,)) => Some(id),
        Err(e) => {
            elog::eprintln!("Bluetooth: cannot show the pairing notification: {e}");
            None
        }
    }
}

fn signal_rule(member: &'static str) -> Option<zbus::MatchRule<'static>> {
    Some(
        zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender(NOTIFY)
            .ok()?
            .interface(NOTIFY)
            .ok()?
            .member(member)
            .ok()?
            .path(NOTIFY_PATH)
            .ok()?
            .build(),
    )
}

/// Which signal answers notification `id`.
fn answer_of(m: &Message, id: u32) -> Option<Answer> {
    let (_, member, _) = crate::sysbus::member(m);
    match member.as_str() {
        "ActionInvoked" => {
            let (i, k) = m.body().deserialize::<(u32, String)>().ok()?;
            (i == id).then_some(Answer::Key(k))
        }
        "NotificationClosed" => {
            let (i, _) = m.body().deserialize::<(u32, u32)>().ok()?;
            (i == id).then_some(Answer::Closed)
        }
        _ => None,
    }
}

/// Ask with Pair/Reject actions on a worker thread (session-bus calls block).
/// What the asking thread blocks on: a notification signal, a Cancel, or the end of the bus.
enum Ev {
    Msg(Message),
    Cancel,
    Gone,
}

/// Owned by the bus subscription; reports its end (the connection closed).
struct Gone(std::sync::mpsc::Sender<Ev>);

impl Drop for Gone {
    fn drop(&mut self) {
        let _ = self.0.send(Ev::Gone);
    }
}

/// Asks the user about a pairing with a notification, on a thread of its own; the answer
/// arrives on the receiver. Where the system refuses the thread nothing is asked and the
/// receiver is closed, which its reader takes as the notification closed unanswered.
fn ask(body: String, accept: &'static str, cancel: Arc<Stop>) -> async_channel::Receiver<Answer> {
    let (tx, rx) = async_channel::bounded(1);
    let ask_user = move || {
        let rules = ["ActionInvoked", "NotificationClosed"]
            .into_iter()
            .filter_map(signal_rule)
            .collect();
        let (etx, sig) = std::sync::mpsc::channel();
        let gone = Gone(etx.clone());
        let sink = Box::new(move |m: &Message| gone.0.send(Ev::Msg(m.clone())).is_ok());
        let Some(conn) = crate::dbus::subscribe_to(crate::dbus::Bus::Session, rules, sink) else {
            elog::eprintln!("Bluetooth: no session bus to ask about pairing");
            let _ = tx.try_send(Answer::Closed);
            return;
        };
        if cancel.load(Ordering::Acquire) {
            let _ = tx.try_send(Answer::Canceled);
            return;
        }
        let Some(id) = notify(
            &conn,
            0,
            &body,
            &["accept", accept, "reject", "Reject"],
            2,
            0,
        ) else {
            let _ = tx.try_send(Answer::Closed);
            return;
        };
        let _cancel = cancel.on_stop(move || {
            let _ = etx.send(Ev::Cancel);
        });
        // bluetoothd ends an unanswered request itself with Agent1.Cancel after its request
        // timeout; if it leaves the bus instead, `bluez_gone` cancels.
        let a = loop {
            if cancel.load(Ordering::Acquire) {
                break Answer::Canceled;
            }
            match sig.recv() {
                Ok(Ev::Msg(m)) => {
                    if let Some(a) = answer_of(&m, id) {
                        break a;
                    }
                }
                Ok(Ev::Cancel) => {}
                Ok(Ev::Gone) | Err(_) => break Answer::Closed,
            }
        };
        if !matches!(a, Answer::Closed) {
            let _ = sysbus::call(
                &conn,
                NOTIFY,
                NOTIFY_PATH,
                NOTIFY,
                "CloseNotification",
                &(id,),
            );
        }
        let _ = tx.try_send(a);
    };
    if let Err(e) = std::thread::Builder::new()
        .name("bluez-ask".into())
        .spawn(ask_user)
    {
        elog::eprintln!("Bluetooth: cannot start a thread to ask about a pairing: {e}");
    }
    rx
}

/// A typed passkey: decimal, 0..=999999.
pub(crate) fn parse_passkey(s: &str) -> Option<u32> {
    let s = s.trim();
    if s.is_empty() || s.len() > 6 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// A typed legacy PIN code: 1 to 16 bytes (the HCI PIN length limit).
pub(crate) fn valid_pin(s: &str) -> bool {
    (1..=16).contains(&s.trim().len())
}

async fn prop(conn: &zbus::Connection, dev: &ObjectPath<'_>, name: &str) -> Option<OwnedValue> {
    let body = (DEVICE, name);
    let get = conn.call_method(Some(BLUEZ), dev, Some(sysbus::PROPS), "Get", &body);
    let r = sysbus::within_async(get).await.ok()?;
    r.body().deserialize::<OwnedValue>().ok()
}

async fn alias(conn: &zbus::Connection, dev: &ObjectPath<'_>) -> String {
    match prop(conn, dev, "Alias")
        .await
        .and_then(|v| String::try_from(v).ok())
    {
        Some(a) if !a.is_empty() => a,
        _ => dev.rsplit('/').next().unwrap_or_default().to_string(),
    }
}

/// Short names of the profiles bluetoothd asks about (Bluetooth SIG Assigned Numbers, 16-bit
/// service class UUIDs on the Base UUID); any other service shows its UUID.
fn service_name(uuid: &str) -> String {
    let short = uuid
        .to_ascii_lowercase()
        .strip_prefix("0000")
        .and_then(|u| u.strip_suffix("-0000-1000-8000-00805f9b34fb"))
        .and_then(|u| u16::from_str_radix(u, 16).ok());
    let name = match short {
        Some(0x1101) => "Serial Port",
        Some(0x1105) => "Object Push",
        Some(0x1106) => "File Transfer",
        Some(0x1108 | 0x1112) => "Headset",
        Some(0x110a) => "Audio Source",
        Some(0x110b) => "Audio Sink",
        Some(0x110c | 0x110e | 0x110f) => "Remote Control",
        Some(0x110d) => "Advanced Audio",
        Some(0x1115) => "Personal Area Network",
        Some(0x1116) => "Network Access Point",
        Some(0x111e | 0x111f) => "Handsfree",
        Some(0x1124 | 0x1812) => "Input Device",
        Some(0x112f) => "Phonebook Access",
        Some(0x1132) => "Message Access",
        _ => return uuid.to_string(),
    };
    name.to_string()
}

/// org.bluez.Agent1 is bluetoothd's callback interface (doc/org.bluez.Agent.rst): only the
/// current owner of org.bluez may call it, whatever the bus policy lets through.
async fn from_bluez(conn: &zbus::Connection, hdr: &Header<'_>) -> Result<(), Error> {
    let owner = match zbus::fdo::DBusProxy::new(conn).await {
        Ok(d) => d
            .get_name_owner(BLUEZ.try_into().map_err(zbus::Error::from)?)
            .await
            .ok(),
        Err(_) => None,
    };
    match (hdr.sender(), owner) {
        (Some(s), Some(o)) if s.as_str() == o.as_str() => Ok(()),
        (s, _) => {
            let who = s.map_or("an unknown sender".to_string(), |s| s.to_string());
            elog::eprintln!(
                "Bluetooth: refused an org.bluez.Agent1 call from {who}, which is not bluetoothd"
            );
            Err(Error::Rejected(
                "Only bluetoothd may call the pairing agent".into(),
            ))
        }
    }
}

impl Agent {
    /// Register a request before its first await.
    fn open(&self) -> Open<'_> {
        let stop = Arc::new(Stop::default());
        self.open.lock().unwrap().push(stop.clone());
        Open { agent: self, stop }
    }

    /// End every open request (its notification and dialog close, and it returns Canceled)
    /// and stop showing a passkey/PIN.
    fn cancel_all(&self) {
        let open = std::mem::take(&mut *self.open.lock().unwrap());
        for c in open {
            c.store(true, Ordering::Release);
        }
        self.hide();
    }

    async fn confirm(
        &self,
        req: &Open<'_>,
        body: String,
        accept: &'static str,
    ) -> Result<(), Error> {
        let a = ask(body, accept, req.stop.clone())
            .recv()
            .await
            .unwrap_or(Answer::Closed);
        match a {
            Answer::Key(k) if k == "accept" => Ok(()),
            Answer::Canceled => Err(Error::Canceled("Canceled".into())),
            _ => Err(Error::Rejected("Rejected by user".into())),
        }
    }

    /// Ask the dialog for a PIN code / passkey for `device`.
    async fn typed(
        &self,
        req: &Open<'_>,
        conn: &zbus::Connection,
        device: &ObjectPath<'_>,
        passkey: bool,
    ) -> Result<String, Error> {
        let what = if passkey { "passkey" } else { "PIN code" };
        let alias = alias(conn, device).await;
        req.live()?;
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let (reply, rx) = async_channel::bounded(1);
        let entry = Entry {
            id,
            passkey,
            alias,
            reply,
        };
        if !to_dialog(Typed::Ask(entry)) {
            elog::eprintln!(
                "Bluetooth: {device} asks for a {what}, but no PolkitAgent dialog can take it"
            );
            return Err(Error::Rejected(format!(
                "Typing a {what} needs the shell's authentication dialog (PolkitAgent), which is not running"
            )));
        }
        // Ended by the dialog, Agent1.Cancel (bluetoothd's request timeout) or `bluez_gone`.
        let _cancel_dialog = req.stop.on_stop(move || {
            to_dialog(Typed::Cancel(id));
        });
        match rx.recv().await {
            Ok(Reply::Key(k)) => Ok(k),
            Ok(Reply::Rejected(m)) => Err(Error::Rejected(m)),
            Ok(Reply::Canceled) | Err(_) => Err(Error::Canceled("Canceled".into())),
        }
    }

    /// Show a code to type on the device (no answer is awaited), unless `req` is cancelled
    /// before it is shown.
    fn show(&self, req: &Open<'_>, body: String) {
        let (shown, stop) = (self.shown.clone(), req.stop.clone());
        DISPLAY.run(move || {
            // A Cancel that came first queued its `hide` before this.
            if stop.load(Ordering::Acquire) {
                return;
            }
            let Some(conn) = crate::dbus::session() else {
                return;
            };
            let mut g = shown.lock().unwrap();
            if let Some(id) = notify(&conn, *g, &body, &[], 1, -1) {
                *g = id;
            }
        });
    }

    /// Stop showing the code to type (org.bluez.Agent1: DisplayPasskey / DisplayPinCode stay
    /// shown until Cancel).
    fn hide(&self) {
        let shown = self.shown.clone();
        DISPLAY.run(move || {
            let id = std::mem::take(&mut *shown.lock().unwrap());
            if id == 0 {
                return;
            }
            let Some(conn) = crate::dbus::session() else {
                return;
            };
            // An error when the user closed it already.
            let _ = sysbus::call(
                &conn,
                NOTIFY,
                NOTIFY_PATH,
                NOTIFY,
                "CloseNotification",
                &(id,),
            );
        });
    }
}

#[zbus::interface(name = "org.bluez.Agent1")]
impl Agent {
    async fn release(
        &self,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(header)] hdr: Header<'_>,
    ) -> Result<(), Error> {
        from_bluez(conn, &hdr).await?;
        self.cancel_all();
        Ok(())
    }

    /// Legacy PIN entry (bluetoothd's autopair plugin already covers well-known fixed PINs).
    async fn request_pin_code(
        &self,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(header)] hdr: Header<'_>,
        device: ObjectPath<'_>,
    ) -> Result<String, Error> {
        let req = self.open();
        from_bluez(conn, &hdr).await?;
        req.live()?;
        self.typed(&req, conn, &device, false).await
    }

    async fn display_pin_code(
        &self,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(header)] hdr: Header<'_>,
        device: ObjectPath<'_>,
        pincode: String,
    ) -> Result<(), Error> {
        let req = self.open();
        from_bluez(conn, &hdr).await?;
        req.live()?;
        let a = alias(conn, &device).await;
        req.live()?;
        self.show(&req, format!("Type {pincode} on {a}, then press Enter"));
        Ok(())
    }

    async fn request_passkey(
        &self,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(header)] hdr: Header<'_>,
        device: ObjectPath<'_>,
    ) -> Result<u32, Error> {
        let req = self.open();
        from_bluez(conn, &hdr).await?;
        req.live()?;
        let k = self.typed(&req, conn, &device, true).await?;
        parse_passkey(&k).ok_or_else(|| Error::Rejected("Invalid passkey".into()))
    }

    async fn display_passkey(
        &self,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(header)] hdr: Header<'_>,
        device: ObjectPath<'_>,
        passkey: u32,
        entered: u16,
    ) -> Result<(), Error> {
        let req = self.open();
        from_bluez(conn, &hdr).await?;
        req.live()?;
        let a = alias(conn, &device).await;
        req.live()?;
        let mut body = format!("Type {passkey:06} on {a}, then press Enter");
        // Called again as keys are typed on the device, with how many are typed so far.
        if entered > 0 {
            body += &format!(" ({entered} of 6 digits typed)");
        }
        self.show(&req, body);
        Ok(())
    }

    async fn request_confirmation(
        &self,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(header)] hdr: Header<'_>,
        device: ObjectPath<'_>,
        passkey: u32,
    ) -> Result<(), Error> {
        let req = self.open();
        from_bluez(conn, &hdr).await?;
        req.live()?;
        let a = alias(conn, &device).await;
        req.live()?;
        self.confirm(
            &req,
            format!("Pair with {a}? Confirm that it shows {passkey:06}"),
            "Pair",
        )
        .await
    }

    async fn request_authorization(
        &self,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(header)] hdr: Header<'_>,
        device: ObjectPath<'_>,
    ) -> Result<(), Error> {
        let req = self.open();
        from_bluez(conn, &hdr).await?;
        req.live()?;
        let a = alias(conn, &device).await;
        req.live()?;
        self.confirm(&req, format!("Pair with {a}?"), "Pair").await
    }

    /// An incoming connection to a profile. bluetoothd asks only for devices that are not
    /// Trusted (org.bluez.Device1.Trusted: the user's "connect without asking"), so the user
    /// decides; unpaired devices are refused without a question.
    async fn authorize_service(
        &self,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(header)] hdr: Header<'_>,
        device: ObjectPath<'_>,
        uuid: String,
    ) -> Result<(), Error> {
        let req = self.open();
        from_bluez(conn, &hdr).await?;
        req.live()?;
        let flag = async |name| {
            prop(conn, &device, name)
                .await
                .and_then(|v| bool::try_from(v).ok())
                .unwrap_or(false)
        };
        let trusted = flag("Trusted").await;
        req.live()?;
        if trusted {
            return Ok(());
        }
        let paired = flag("Paired").await;
        req.live()?;
        if !paired {
            elog::eprintln!("Bluetooth: rejected service {uuid} for unpaired {device}");
            return Err(Error::Rejected("Device is not paired".into()));
        }
        let a = alias(conn, &device).await;
        req.live()?;
        let svc = service_name(&uuid);
        self.confirm(&req, format!("Allow {a} to use {svc}?"), "Allow")
            .await
    }

    async fn cancel(
        &self,
        #[zbus(connection)] conn: &zbus::Connection,
        #[zbus(header)] hdr: Header<'_>,
    ) -> Result<(), Error> {
        // The requests open when bluetoothd sent this: one it sends after does not end with them.
        let open = self.open.lock().unwrap().clone();
        from_bluez(conn, &hdr).await?;
        for c in open {
            c.store(true, Ordering::Release);
        }
        self.hide();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_passkey, service_name, signal_rule, valid_pin};

    #[test]
    fn pairing_answers_only_from_the_notification_server() {
        // Notification ids are sequential: without a sender, any session-bus client could
        // send `ActionInvoked(id, "accept")` and confirm someone else's pairing.
        for m in ["ActionInvoked", "NotificationClosed"] {
            let r = signal_rule(m).unwrap();
            assert_eq!(
                r.sender().map(|s| s.to_string()).as_deref(),
                Some("org.freedesktop.Notifications"),
                "{m}"
            );
        }
    }

    #[test]
    fn services_are_named_by_their_assigned_number() {
        assert_eq!(
            service_name("0000110D-0000-1000-8000-00805F9B34FB"),
            "Advanced Audio"
        );
        assert_eq!(
            service_name("00001124-0000-1000-8000-00805f9b34fb"),
            "Input Device"
        );
        assert_eq!(
            service_name("00001812-0000-1000-8000-00805f9b34fb"),
            "Input Device"
        );
        let custom = "6e400001-b5a3-f393-e0a9-e50e24dcca9e";
        assert_eq!(service_name(custom), custom);
        assert_eq!(
            service_name("0000fffe-0000-1000-8000-00805f9b34fb"),
            "0000fffe-0000-1000-8000-00805f9b34fb"
        );
    }

    #[test]
    fn typed_keys_validate_like_bluez() {
        assert_eq!(parse_passkey(" 004321 "), Some(4321));
        assert_eq!(parse_passkey("0"), Some(0));
        assert_eq!(parse_passkey("999999"), Some(999999));
        for bad in ["", "1000000", "0000000", "-1", "+1", "12a4", "1 2"] {
            assert_eq!(parse_passkey(bad), None, "{bad:?}");
        }
        assert!(valid_pin("0000"));
        assert!(valid_pin("0123456789abcdef"));
        assert!(!valid_pin("  "));
        assert!(!valid_pin("0123456789abcdefg"));
    }

    /// The system refuses the thread that asks the user about a pairing (the limit on threads
    /// is reached): `ask` returns, instead of panicking, a closed receiver, which the agent
    /// answers bluetoothd with as it does a notification closed unanswered (rejected). The
    /// refusal is in the log. In a process of its own: the test reads what that process wrote
    /// to its stderr.
    #[test]
    fn a_refused_thread_leaves_the_pairing_unanswered() {
        const TEST: &str = concat!(
            module_path!(),
            "::a_refused_thread_leaves_the_pairing_unanswered"
        );
        if !crate::testenv::in_own_process(TEST) {
            crate::testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
            return;
        }
        crate::testenv::refuse_new_threads();
        let (rx, log) =
            crate::testenv::stderr_with(|| super::ask("Pair?".into(), "Pair", Default::default()));
        assert!(
            log.contains("Bluetooth: cannot start a thread to ask about a pairing:"),
            "stderr: {log:?}"
        );
        assert!(matches!(
            rx.try_recv(),
            Err(async_channel::TryRecvError::Closed)
        ));
    }

    /// The system refuses the thread that registers the agent again when the dialog comes or
    /// goes: `reregister` returns instead of panicking, and the refusal is in the log. In a
    /// process of its own: the test reads what that process wrote to its stderr.
    #[test]
    fn a_refused_thread_keeps_the_registered_capability() {
        const TEST: &str = concat!(
            module_path!(),
            "::a_refused_thread_keeps_the_registered_capability"
        );
        if !crate::testenv::in_own_process(TEST) {
            crate::testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
            return;
        }
        crate::testenv::refuse_new_threads();
        let ((), log) = crate::testenv::stderr_with(super::reregister);
        assert!(
            log.contains("Bluetooth: cannot start a thread to register the pairing agent:"),
            "stderr: {log:?}"
        );
    }
}
