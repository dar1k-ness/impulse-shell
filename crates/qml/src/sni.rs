//! Native `Rustshell.Services.SystemTray`: a StatusNotifierWatcher (when no other one owns the
//! name, like Quickshell's) plus a host that mirrors every registered item's properties.
use crate::runtime::{Runtime, World};
use crate::value::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use zbus::MatchRule;
use zbus::blocking::Connection;
use zbus::message::{Sequence, Type as MsgType};
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedValue, Value as ZV};

const WATCHER: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const ITEM: &str = "org.kde.StatusNotifierItem";
const MENU: &str = "com.canonical.dbusmenu";

/// How long a menu's app has to answer `AboutToShow`, `GetLayout` and `GetGroupProperties`:
/// the 25 s Qt gives a D-Bus call by default (`QDBusAbstractInterface::timeout()` is -1,
/// libdbus's own default), which Quickshell's dbusmenu calls run under.
const MENU_REPLY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(25);

/// Held by whoever wants a menu fetch's outcome (the opener, while it shows that key): dropped,
/// it ends the fetch's call in flight, so its lane goes on to the next job.
type Wanted = async_channel::Sender<std::convert::Infallible>;
/// The fetch's side of [`Wanted`].
type Unwanted = async_channel::Receiver<std::convert::Infallible>;

/// One `com.canonical.dbusmenu` entry as Quickshell's `DBusMenuItem` exposes it.
#[derive(Clone, PartialEq, Default)]
pub struct MEntry {
    pub id: i32,
    pub separator: bool,
    pub enabled: bool,
    pub visible: bool,
    pub text: String,
    pub icon: String,
    pub button: u8,
    pub check: u8,
    pub children: bool,
}

/// A `com.canonical.dbusmenu` signal of a menu some opener shows.
enum MenuSig {
    /// `LayoutUpdated(revision, parent)`: `parent`'s subtree changed (0: the whole layout).
    Layout { parent: i32 },
    /// `ItemsPropertiesUpdated(updated, removed)`.
    Props {
        updated: Vec<(i32, HashMap<String, OwnedValue>)>,
        removed: Vec<(i32, Vec<String>)>,
    },
}

/// A menu signal with its menu (`svc|path`) and its receive position on the shared session
/// connection, which orders it against the replies of the fetches on that connection.
struct MenuMsg {
    menu: String,
    at: Sequence,
    sig: Arc<MenuSig>,
}

/// One menu level as fetched: the children of the level's entry with their properties, and the
/// receive positions of the GetLayout and GetGroupProperties replies they come from.
struct Level {
    entries: Vec<(i32, HashMap<String, OwnedValue>)>,
    layout_at: Sequence,
    props_at: Sequence,
}

/// A shown entry: its `QsMenuEntry` and the DBusMenu properties it was built from.
struct Shown {
    item: usize,
    entry: MEntry,
    props: HashMap<String, OwnedValue>,
}

/// A `QsMenuOpener`'s menu level.
#[derive(Default)]
struct Opener {
    /// `svc|path|id` of the handle it shows, empty without one.
    key: String,
    shown: Vec<Shown>,
    /// Receive positions of the replies `shown` comes from (`None`: nothing fetched for `key`).
    layout_at: Option<Sequence>,
    props_at: Option<Sequence>,
    /// The key and generation of the fetch in flight: a reply of any other fetch (another key,
    /// or an earlier fetch of this one) is never applied. Cleared, it also ends that fetch.
    fetching: Option<(String, u64, Wanted)>,
    /// The shown layout is older than a `LayoutUpdated` that covers it.
    stale: bool,
    /// Signals of `key`'s menu that arrived while its fetch was in flight, to replay against
    /// the reply positions once it lands.
    held: Vec<(Sequence, Arc<MenuSig>)>,
}

/// ARGB32 (network byte order) icon pixmap as sent over D-Bus.
#[derive(Clone, Debug, PartialEq)]
pub struct Pix {
    pub w: i32,
    pub h: i32,
    pub data: Vec<u8>,
}

#[derive(Clone, PartialEq, Default)]
pub struct Snap {
    pub key: String,
    /// The unique name of the bus name's owner the item was read from: its menu is asked there,
    /// so a menu of an earlier owner is never shown for the item.
    pub owner: String,
    pub id: String,
    pub title: String,
    pub status: u8,
    pub category: u8,
    pub icon_name: String,
    pub theme_path: String,
    pub attention_name: String,
    pub overlay_name: String,
    pub tip_title: String,
    pub tip_desc: String,
    pub menu: String,
    pub only_menu: bool,
    pub pixmaps: Arc<Vec<Pix>>,
    pub attention_pixmaps: Arc<Vec<Pix>>,
    pub overlay_pixmaps: Arc<Vec<Pix>>,
}

struct Watcher {
    items: Arc<Mutex<Vec<String>>>,
    /// Who registered each key, for [`MAX_ITEMS_PER_SENDER`].
    by: Mutex<HashMap<String, String>>,
    hosts: Mutex<Hosts>,
}

/// The hosts registered with the [`Watcher`], each the bus name it registered with the unique
/// name that owned it then: a registration ends when that owner lets the name go, so a host that
/// restarts (same name, new owner) registers, and is announced, anew.
#[derive(Default)]
struct Hosts {
    list: Vec<(String, String)>,
    /// The `IsStatusNotifierHostRegistered` value last announced.
    announced: bool,
}

impl Hosts {
    /// The `IsStatusNotifierHostRegistered` value to announce, if it differs from the last one.
    fn change(&mut self) -> Option<bool> {
        let now = !self.list.is_empty();
        (now != self.announced).then(|| {
            self.announced = now;
            now
        })
    }
}

/// Items one connection may have registered at a time.
const MAX_ITEMS_PER_SENDER: usize = 16;

/// Pixmaps of one `a(iiay)` property kept (apps send a handful of sizes).
const MAX_PIXMAPS: usize = 16;

async fn has_owner(conn: &zbus::Connection, bus: &str) -> bool {
    conn.call_method(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        Some("org.freedesktop.DBus"),
        "NameHasOwner",
        &(bus,),
    )
    .await
    .ok()
    .and_then(|r| r.body().deserialize::<bool>().ok())
    .unwrap_or(false)
}

/// The unique name that owns `bus`, if any.
async fn owner_of(conn: &zbus::Connection, bus: &str) -> Option<String> {
    conn.call_method(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        Some("org.freedesktop.DBus"),
        "GetNameOwner",
        &(bus,),
    )
    .await
    .ok()
    .and_then(|r| r.body().deserialize::<String>().ok())
}

impl Watcher {
    /// `name` went from owner `old` to `new` (either "" for none): the items registered under a
    /// name nobody owns any more, and the hosts registered by `old`, are gone.
    async fn owner_changed(&self, em: &SignalEmitter<'_>, name: &str, old: &str, new: &str) {
        let items: Vec<String> = if new.is_empty() {
            let mut v = self.items.lock().unwrap();
            let gone = v
                .iter()
                .filter(|k| split_key(k).0 == name)
                .cloned()
                .collect();
            v.retain(|k| split_key(k).0 != name);
            gone
        } else {
            vec![]
        };
        for k in &items {
            let _ = Self::status_notifier_item_unregistered(em, k).await;
        }
        if !items.is_empty() {
            let _ = self.registered_status_notifier_items_changed(em).await;
        }
        let (hosts, change) = {
            let mut h = self.hosts.lock().unwrap();
            let before = h.list.len();
            h.list.retain(|(s, o)| s != name || o != old);
            (before - h.list.len(), h.change())
        };
        for _ in 0..hosts {
            let _ = Self::status_notifier_host_unregistered(em).await;
        }
        if change.is_some() {
            let _ = self.is_status_notifier_host_registered_changed(em).await;
        }
    }
}

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    async fn register_status_notifier_item(
        &self,
        service: &str,
        #[zbus(header)] hdr: zbus::message::Header<'_>,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<()> {
        let sender = hdr.sender().map(|s| s.to_string()).unwrap_or_default();
        let key = if service.starts_with('/') {
            format!("{sender}{service}")
        } else {
            format!("{service}/StatusNotifierItem")
        };
        {
            let mut v = self.items.lock().unwrap();
            if v.contains(&key) {
                return Ok(());
            }
            let mut by = self.by.lock().unwrap();
            by.retain(|k, _| v.contains(k));
            if by.values().filter(|s| **s == sender).count() >= MAX_ITEMS_PER_SENDER {
                return Err(zbus::fdo::Error::LimitsExceeded(format!(
                    "{sender} has registered {MAX_ITEMS_PER_SENDER} items"
                )));
            }
            v.push(key.clone());
            by.insert(key.clone(), sender);
        }
        // Only a name someone owns is an item: a key is dropped when its name loses its owner,
        // so one that never had one would stay forever. Checked once the key is listed, so a
        // NameOwnerChanged from here on (the subscription's sink) removes it as well.
        let bus = split_key(&key).0.to_string();
        if !has_owner(conn, &bus).await {
            self.items.lock().unwrap().retain(|k| *k != key);
            return Err(zbus::fdo::Error::NameHasNoOwner(format!(
                "{bus} has no owner"
            )));
        }
        let _ = Self::status_notifier_item_registered(&em, &key).await;
        let _ = self.registered_status_notifier_items_changed(&em).await;
        Ok(())
    }

    async fn register_status_notifier_host(
        &self,
        service: &str,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<()> {
        let no_owner = || zbus::fdo::Error::NameHasNoOwner(format!("{service} has no owner"));
        let owner = owner_of(conn, service).await.ok_or_else(no_owner)?;
        let entry = (service.to_string(), owner);
        {
            let mut h = self.hosts.lock().unwrap();
            if h.list.contains(&entry) {
                return Ok(());
            }
            h.list.push(entry.clone());
        }
        // Like an item's: checked again once listed, so the owner's NameOwnerChanged from here
        // on (the subscription's sink) ends the registration, and an earlier one is seen here.
        if owner_of(conn, service).await.as_ref() != Some(&entry.1) {
            let change = {
                let mut h = self.hosts.lock().unwrap();
                h.list.retain(|e| *e != entry);
                h.change()
            };
            if change.is_some() {
                let _ = self.is_status_notifier_host_registered_changed(&em).await;
            }
            return Err(no_owner());
        }
        let _ = Self::status_notifier_host_registered(&em).await;
        let change = self.hosts.lock().unwrap().change();
        if change.is_some() {
            let _ = self.is_status_notifier_host_registered_changed(&em).await;
        }
        Ok(())
    }

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.items.lock().unwrap().clone()
    }

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        !self.hosts.lock().unwrap().list.is_empty()
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }

    #[zbus(signal)]
    async fn status_notifier_item_registered(
        em: &SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn status_notifier_item_unregistered(
        em: &SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn status_notifier_host_registered(em: &SignalEmitter<'_>) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn status_notifier_host_unregistered(em: &SignalEmitter<'_>) -> zbus::Result<()>;
}

/// A menu fetch's outcome: opener, key, fetch generation, and the level (`None`: it failed).
type MenuReply = (usize, String, u64, Option<Level>);

pub struct Sni {
    rx: Receiver<Vec<Snap>>,
    /// The service thread's connection, replaced when it reconnects.
    conn: Arc<Mutex<Option<Connection>>>,
    /// Unique name of the connection `menu_subs` and the shown menus were fetched on.
    menu_conn: Option<String>,
    /// (key, item, last snapshot, pixmap generation) in registration order.
    items: Vec<(String, usize, Snap, u64)>,
    pixgen: u64,
    menus: HashMap<usize, Opener>,
    /// Menu keys some opener holds: DBusMenu `opened`/`closed` events and `AboutToShow`
    /// follow its changes.
    menus_open: std::collections::HashSet<String>,
    /// The signal subscription of every menu (`svc|path`) some opener shows, ended (and its
    /// match removed from the bus) when the last such opener goes or the tray is dropped.
    menu_subs: HashMap<String, MenuSub>,
    /// Entry id -> parent id of every level fetched per shown menu, for `LayoutUpdated`'s
    /// subtree.
    menu_parents: HashMap<String, HashMap<i32, i32>>,
    /// Calls to each menu (`svc|path`), in the order the tray makes them: `opened` before
    /// `closed`, `closed` before the next `opened`, `clicked` in between.
    menu_lanes: HashMap<String, Arc<workpool::Lane>>,
    /// Keys opened whose `opened` event has not gone out yet: it goes with their next fetch.
    menus_opening: std::collections::HashSet<String>,
    /// Keys whose `opened` went out and whose `closed` has not, written on their menu's lane
    /// as the events go out: `closed` only ends an `opened` the app got.
    menus_announced: Arc<Mutex<std::collections::HashSet<String>>>,
    /// Generation of the last menu fetch started.
    menu_gen: u64,
    stx: crate::svc::Sender<MenuMsg>,
    srx: Receiver<MenuMsg>,
    mtx: crate::svc::Sender<MenuReply>,
    mrx: Receiver<MenuReply>,
    /// The fetch loop's events, for [`Ev::Stop`].
    tick: Sender<Ev>,
    fetch: Arc<Mutex<FetchSub>>,
    /// Set with the tray's drop: ends the fetch thread's wait for the bus socket.
    stop: Arc<crate::svc::Stop>,
}

/// The fetch loop's subscription on its current connection. The tray's drop ends it at once:
/// left to the loop, its sink and matches would stay routed until a matching signal found the
/// loop gone.
#[derive(Default)]
struct FetchSub {
    stopped: bool,
    cancel: Option<crate::dbus::Cancel>,
}

impl Drop for Sni {
    fn drop(&mut self) {
        // Before the sink goes: the loop then reads the stop ahead of the `Ev::Closed` that
        // follows, and does not serve again.
        let _ = self.tick.send(Ev::Stop);
        let cancel = {
            let mut f = self.fetch.lock().unwrap_or_else(|e| e.into_inner());
            f.stopped = true;
            f.cancel.take()
        };
        if let Some(c) = cancel {
            c.cancel();
        }
        self.stop.store(true, Ordering::Release);
    }
}

/// What each item shows, by item key, for `image://service.sni/<key>/<gen>` sources.
static PIXMAPS: Mutex<Option<HashMap<String, Look>>> = Mutex::new(None);

/// Where one layer of a tray icon comes from.
#[derive(Clone, Debug, PartialEq)]
enum Src {
    Pixmaps(Arc<Vec<Pix>>),
    /// An `image://icon/` source.
    Named(String),
}

/// The icon an item shows: its base (attention or normal icon) and the overlay painted over it.
#[derive(Clone, Debug, PartialEq)]
struct Look {
    base: Src,
    overlay: Option<Src>,
}

fn split_key(key: &str) -> (&str, String) {
    match key.find('/') {
        Some(i) => (&key[..i], key[i..].to_string()),
        None => (key, "/StatusNotifierItem".into()),
    }
}

/// Icon pixmaps of an `a(iiay)` value. They arrive typed (one byte vector each, not one `Value`
/// per byte); a pixmap whose data is shorter than its size says is dropped. One up to
/// [`MAX_PIX_SIDE`] is kept as it came, as Quickshell keeps it (a 512x512 pixmap asked for at
/// 512x512 is drawn pixel for pixel: tests/tray_pixels_qt.rs in the shell); a larger one is
/// kept averaged down, so an item cannot make the tray hold (and copy on every texture load)
/// arbitrarily large images.
fn pixmaps(v: Option<&crate::dbusvar::Var>) -> Arc<Vec<Pix>> {
    let raw = match v {
        Some(crate::dbusvar::Var::Pixmaps(p)) => p,
        _ => return Default::default(),
    };
    let max = MAX_PIX_SIDE as usize;
    let out = raw
        .iter()
        .filter_map(|(w, h, crate::dbusvar::Bytes(d))| {
            let (w, h) = (usize::try_from(*w).ok()?, usize::try_from(*h).ok()?);
            let n = w.checked_mul(h)?.checked_mul(4)?;
            if n == 0 || d.len() < n {
                return None;
            }
            if w <= max && h <= max {
                return Some(Pix {
                    w: w as i32,
                    h: h as i32,
                    data: d[..n].to_vec(),
                });
            }
            let (tw, th, px) = crate::dbusvar::fit(w, h, max, |x, y| {
                let c = &d[(y * w + x) * 4..][..4];
                let pm = |v: u8| mul255::qt(v as u32, c[0] as u32) as u8;
                [pm(c[1]), pm(c[2]), pm(c[3]), c[0]]
            });
            let data = px
                .iter()
                .flat_map(|&[r, g, b, a]| {
                    let un = |v: u8| {
                        if a == 0 {
                            0
                        } else {
                            ((v as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8
                        }
                    };
                    [a, un(r), un(g), un(b)]
                })
                .collect();
            Some(Pix {
                w: tw as i32,
                h: th as i32,
                data,
            })
        })
        .take(MAX_PIXMAPS)
        .collect();
    Arc::new(out)
}

/// Largest side of a stored pixmap that is drawn. [`pixmaps`] already keeps every side within
/// it, so this is a second check where the bytes are read: a size outside it has no data to
/// draw (and, unchecked, overflowed the `w * h * 4` size).
const MAX_PIX_SIDE: i32 = crate::dbusvar::MAX_SIDE as i32;

/// Bytes of ARGB32 data a pixmap's `w`x`h` needs, if the size is sane.
fn pix_len(p: &Pix) -> Option<usize> {
    let side = 1..=MAX_PIX_SIDE;
    if !side.contains(&p.w) || !side.contains(&p.h) {
        return None;
    }
    (p.w as usize).checked_mul(p.h as usize)?.checked_mul(4)
}

/// Quickshell `Category::Enum`: Hardware 0, SystemServices 1, ApplicationStatus 2 (also the
/// default), Communications 3.
fn category(wire: &str) -> u8 {
    match wire {
        "Hardware" => 0,
        "SystemServices" => 1,
        "Communications" => 3,
        _ => 2,
    }
}

/// Every `org.kde.StatusNotifierItem` property a [`Snap`] mirrors.
const PROPS: [&str; 14] = [
    "Id",
    "Title",
    "Status",
    "Category",
    "IconName",
    "IconThemePath",
    "AttentionIconName",
    "OverlayIconName",
    "ToolTip",
    "Menu",
    "ItemIsMenu",
    "IconPixmap",
    "AttentionIconPixmap",
    "OverlayIconPixmap",
];

fn status(wire: &str) -> u8 {
    match wire {
        "Active" => 1,
        "NeedsAttention" => 2,
        _ => 0,
    }
}

/// Property `name` of `s` from its D-Bus value (`None`: the item does not have it).
fn set_prop(s: &mut Snap, name: &str, var: Option<&crate::dbusvar::Var>) {
    let v = match var {
        Some(crate::dbusvar::Var::Value(v)) => Some(unwrap_v(v)),
        _ => None,
    };
    let st = || match v {
        Some(ZV::Str(s)) => s.to_string(),
        Some(ZV::ObjectPath(p)) => p.to_string(),
        _ => String::new(),
    };
    match name {
        "Id" => s.id = st(),
        "Title" => s.title = st(),
        "Status" => s.status = status(&st()),
        "Category" => s.category = category(&st()),
        "IconName" => s.icon_name = st(),
        "IconThemePath" => s.theme_path = st(),
        "AttentionIconName" => s.attention_name = st(),
        "OverlayIconName" => s.overlay_name = st(),
        "ToolTip" => {
            (s.tip_title, s.tip_desc) = match (var, v) {
                (Some(crate::dbusvar::Var::ToolTip(t, d)), _) => (t.clone(), d.clone()),
                (_, Some(ZV::Structure(st))) => match st.fields() {
                    [_, _, ZV::Str(t), ZV::Str(d)] => (t.to_string(), d.to_string()),
                    _ => Default::default(),
                },
                _ => Default::default(),
            }
        }
        "Menu" => s.menu = st(),
        "ItemIsMenu" => s.only_menu = matches!(v, Some(ZV::Bool(true))),
        "IconPixmap" => s.pixmaps = pixmaps(var),
        "AttentionIconPixmap" => s.attention_pixmaps = pixmaps(var),
        "OverlayIconPixmap" => s.overlay_pixmaps = pixmaps(var),
        _ => {}
    }
}

/// `GetAll` of item `key` from `owner`, the unique name owning its bus name.
async fn fetch_item(conn: &zbus::Connection, owner: &str, key: &str) -> Option<Snap> {
    let (_, path) = split_key(key);
    let reply = conn
        .call_method(
            Some(owner),
            path.as_str(),
            Some("org.freedesktop.DBus.Properties"),
            "GetAll",
            &(ITEM,),
        )
        .await
        .map_err(|e| elog::eprintln!("sni: GetAll {key}: {e}"))
        .ok()?;
    let m: HashMap<String, crate::dbusvar::Var> = reply
        .body()
        .deserialize()
        .map_err(|e| elog::eprintln!("sni: GetAll {key} body: {e}"))
        .ok()?;
    let mut s = Snap {
        key: key.to_string(),
        owner: owner.to_string(),
        ..Default::default()
    };
    for p in PROPS {
        set_prop(&mut s, p, m.get(p));
    }
    Some(s)
}

/// Errors the bus daemon answers for a peer that is gone (or left without replying): they say
/// nothing about the property asked.
fn peer_gone(e: &zbus::Error) -> bool {
    matches!(e, zbus::Error::MethodError(n, _, _) if matches!(
        n.as_str(),
        "org.freedesktop.DBus.Error.NoReply"
            | "org.freedesktop.DBus.Error.ServiceUnknown"
            | "org.freedesktop.DBus.Error.NameHasNoOwner"
    ))
}

/// Properties `names` of item `key` read one by one from `owner`: a property the item answers
/// with an error is unset (`None`), as `GetAll` would omit it; one it never answers (gone, or a
/// broken reply) is left out, keeping the old value.
async fn fetch_props(
    conn: &zbus::Connection,
    owner: &str,
    key: &str,
    names: &[&'static str],
) -> Vec<(&'static str, Option<crate::dbusvar::Var>)> {
    let (_, path) = split_key(key);
    let mut out = vec![];
    for &name in names {
        let r = conn
            .call_method(
                Some(owner),
                path.as_str(),
                Some("org.freedesktop.DBus.Properties"),
                "Get",
                &(ITEM, name),
            )
            .await;
        match r {
            Ok(m) => match m.body().deserialize::<crate::dbusvar::Var>() {
                Ok(v) => out.push((name, Some(v))),
                Err(e) => elog::eprintln!("sni: Get {key} {name} body: {e}"),
            },
            Err(e) if peer_gone(&e) => break,
            Err(zbus::Error::MethodError(..)) => out.push((name, None)),
            Err(e) => elog::eprintln!("sni: Get {key} {name}: {e}"),
        }
    }
    out
}

/// The item was read whole (it registered, or a signal that names no property) or said its
/// icons changed (`NewIcon`, `NewAttentionIcon`, `NewOverlayIcon`, `NewIconThemePath`): icon
/// names not found in its own theme directory before are searched again when next drawn.
fn icons_changed(s: &Snap) {
    if !s.theme_path.is_empty() {
        crate::icons::own_theme_changed(&s.theme_path);
    }
}

/// What an item signal says changed (Quickshell's per-signal property updates): `None` for a
/// signal the spec does not tie to properties, which refetches the whole item.
fn signal_props(member: &str) -> Option<&'static [&'static str]> {
    Some(match member {
        "NewTitle" => &["Title"],
        "NewIcon" => &["IconName", "IconPixmap", "IconThemePath"],
        "NewAttentionIcon" => &["AttentionIconName", "AttentionIconPixmap"],
        "NewOverlayIcon" => &["OverlayIconName", "OverlayIconPixmap"],
        "NewToolTip" => &["ToolTip"],
        "NewStatus" => &["Status"],
        "NewMenu" => &["Menu", "ItemIsMenu"],
        "NewIconThemePath" => &["IconThemePath"],
        _ => return None,
    })
}

/// What the signal thread hands the fetch thread.
enum Ev {
    /// Items may have been registered or unregistered.
    List,
    /// Item signal `member` from object `path` of unique bus name `sender`; `status` is the
    /// value a `NewStatus` carries.
    Item {
        sender: String,
        path: String,
        member: String,
        status: Option<String>,
    },
    /// Bus name `name` is now owned by unique name `new` (empty: nobody).
    Owner { name: String, new: String },
    /// Fetch `id` of item `key` from `owner` (`None`: the name had none) is done.
    Fetched {
        key: String,
        id: u64,
        owner: Option<String>,
        got: Got,
    },
    /// Listing `id` of a foreign Watcher's `RegisteredStatusNotifierItems` is done.
    Listed { id: u64, keys: Vec<String> },
    /// The router dropped the fetch loop's sink of connection generation `.0`: that
    /// connection closed.
    Closed(u64),
    /// The tray's runtime is gone: end the fetch loop and its subscription.
    Stop,
}

/// What an item fetch read.
enum Got {
    /// The whole item (`None`: it did not answer).
    Item(Option<Box<Snap>>),
    /// Some of its properties, as [`fetch_props`] reads them.
    Props(Vec<(&'static str, Option<crate::dbusvar::Var>)>),
}

/// Changes of one item a coalesced batch of signals asks for.
#[derive(Default)]
struct Dirty {
    all: bool,
    props: Vec<&'static str>,
    status: Option<String>,
}

/// The unique name owning `bus`, asked of the bus daemon (which always answers).
fn name_owner(conn: &Connection, bus: &str) -> Option<String> {
    if bus.starts_with(':') {
        return Some(bus.to_string());
    }
    conn.call_method(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        Some("org.freedesktop.DBus"),
        "GetNameOwner",
        &(bus,),
    )
    .ok()
    .and_then(|r| r.body().deserialize::<String>().ok())
}

/// A foreign Watcher's registrations (none if it does not answer).
async fn registered(conn: &zbus::Connection) -> Vec<String> {
    conn.call_method(
        Some(WATCHER),
        WATCHER_PATH,
        Some("org.freedesktop.DBus.Properties"),
        "Get",
        &(WATCHER, "RegisteredStatusNotifierItems"),
    )
    .await
    .ok()
    .and_then(|r| r.body().deserialize::<OwnedValue>().ok())
    .and_then(|v| Vec::<String>::try_from(v).ok())
    .unwrap_or_default()
}

/// Calls to other clients run as tasks on the connection's executor, never blocking a thread: a
/// client that does not answer holds only its task, until the bus answers for it (`NoReply` once
/// it disconnects) or the task is dropped.
fn task<T: Send + 'static>(
    conn: &Connection,
    name: &str,
    f: impl std::future::Future<Output = T> + Send + 'static,
) -> zbus::Task<T> {
    conn.inner().executor().spawn(f, name)
}

/// Request the Watcher name for the Watcher [`watcher_items`] serves on `conn` (`items`, its
/// registrations); `items` while `conn` owns the name. The shared connection may hold the name
/// already, from an earlier tray of this process: it names that same Watcher.
fn claim_watcher(
    conn: &Connection,
    items: &Arc<Mutex<Vec<String>>>,
) -> Option<Arc<Mutex<Vec<String>>>> {
    let own = conn
        .request_name_with_flags(WATCHER, zbus::fdo::RequestNameFlags::DoNotQueue.into())
        .is_ok_and(|r| {
            matches!(
                r,
                zbus::fdo::RequestNameReply::PrimaryOwner
                    | zbus::fdo::RequestNameReply::AlreadyOwner
            )
        });
    if crate::log_on!("ri::sni") {
        tracing::debug!(target: "ri::sni", "sni: own watcher = {own}");
    }
    own.then(|| items.clone())
}

/// `RegisterStatusNotifierHost(host)` with whoever owns the Watcher name.
fn register_host(conn: &Connection, host: &str) {
    let (c, host) = (conn.inner().clone(), host.to_string());
    task(conn, "sni RegisterStatusNotifierHost", async move {
        let _ = c
            .call_method(
                Some(WATCHER),
                WATCHER_PATH,
                Some(WATCHER),
                "RegisterStatusNotifierHost",
                &(host,),
            )
            .await;
    })
    .detach();
}

/// A bus name the connection owns until this is dropped.
struct OwnedName(Connection, String);

impl Drop for OwnedName {
    fn drop(&mut self) {
        let _ = self.0.release_name(self.1.as_str());
    }
}

fn rules() -> zbus::Result<Vec<MatchRule<'static>>> {
    let sig = || MatchRule::builder().msg_type(MsgType::Signal);
    Ok(vec![
        sig().interface(ITEM)?.build(),
        sig().interface(WATCHER)?.build(),
        sig()
            .sender("org.freedesktop.DBus")?
            .interface("org.freedesktop.DBus")?
            .member("NameOwnerChanged")?
            .build(),
    ])
}

fn start(h: &crate::svc::Handle) -> Option<Sni> {
    let (tx, rx) = h.channel();
    let conn = Arc::new(Mutex::new(None));
    let c = conn.clone();
    let (tick, ticks) = channel::<Ev>();
    let t = tick.clone();
    let fetch = Arc::new(Mutex::new(FetchSub::default()));
    let f = fetch.clone();
    let stop = Arc::new(crate::svc::Stop::default());
    let st = stop.clone();
    // Connecting, AddMatch, the Watcher and host names all wait on other processes: the main
    // loop only gets snapshots.
    if let Err(e) = std::thread::Builder::new()
        .name("sni".into())
        .spawn(move || serve(c, f, tx, t, ticks, st))
    {
        // Refused by the system (`EAGAIN`): no tray now; `poll_sni` starts it again.
        static SAID: std::sync::Once = std::sync::Once::new();
        SAID.call_once(|| elog::eprintln!("SystemTray: cannot start its thread: {e}"));
        return None;
    }
    let (mtx, mrx) = h.channel();
    let (stx, srx) = h.channel();
    Some(Sni {
        rx,
        conn,
        menu_conn: None,
        items: vec![],
        pixgen: 0,
        menus: HashMap::new(),
        menus_open: Default::default(),
        menu_subs: HashMap::new(),
        menu_parents: HashMap::new(),
        menu_lanes: HashMap::new(),
        menus_opening: Default::default(),
        menus_announced: Default::default(),
        menu_gen: 0,
        stx,
        srx,
        mtx,
        mrx,
        tick,
        fetch,
        stop,
    })
}

/// The registrations of the Watcher served on `conn`, serving it first if this is the first
/// tray on that connection. The Watcher belongs to the connection, which keeps its bus name
/// across trays of this process: a later tray (no `NameOwnerChanged` for the name, so no item
/// re-registers) must list what the earlier one had. Its registrations end when their name
/// loses its owner, tray or not; that subscription precedes every tray's (routed in order), so
/// a tray's fetch loop never reads a key whose owner it has already seen go.
fn watcher_items(conn: &Connection) -> Option<Arc<Mutex<Vec<String>>>> {
    static SERVING: Mutex<()> = Mutex::new(());
    let _one = SERVING.lock().unwrap_or_else(|e| e.into_inner());
    if let Ok(w) = conn.object_server().interface::<_, Watcher>(WATCHER_PATH) {
        return Some(w.get().items.clone());
    }
    let list = Arc::new(Mutex::new(Vec::<String>::new()));
    let rule = MatchRule::builder()
        .msg_type(MsgType::Signal)
        .sender("org.freedesktop.DBus")
        .and_then(|b| b.interface("org.freedesktop.DBus"))
        .and_then(|b| b.member("NameOwnerChanged"))
        .map_err(|e| elog::eprintln!("sni: rules: {e}"))
        .ok()?
        .build();
    let c = conn.clone();
    let _ = crate::dbus::subscribe_to(
        crate::dbus::Bus::Session,
        vec![rule],
        Box::new(move |m| {
            // A name nobody owned before ends no registration.
            if let Ok((name, old, new)) = m.body().deserialize::<(String, String, String)>()
                && !old.is_empty()
                && let Ok(w) = c.object_server().interface::<_, Watcher>(WATCHER_PATH)
            {
                zbus::block_on(w.get().owner_changed(w.signal_emitter(), &name, &old, &new));
            }
            true
        }),
    )?;
    let watcher = Watcher {
        items: list.clone(),
        by: Default::default(),
        hosts: Default::default(),
    };
    if let Err(e) = conn.object_server().at(WATCHER_PATH, watcher) {
        elog::eprintln!("sni: serve watcher: {e}");
        return None;
    }
    Some(list)
}

/// How the tray's time on one connection ended.
enum End {
    /// The tray is gone.
    Stopped,
    /// The connection closed.
    Closed,
    /// There was no connection to serve on: the session bus is not up.
    NoBus,
}

/// The tray's bus side: watcher, host and the item fetch loop, on one connection after another.
fn serve(
    conn_out: Arc<Mutex<Option<Connection>>>,
    fetch: Arc<Mutex<FetchSub>>,
    tx: crate::svc::Sender<Vec<Snap>>,
    tick: Sender<Ev>,
    ticks: Receiver<Ev>,
    stop: Arc<crate::svc::Stop>,
) {
    // Set up before the first connect, so a socket created in between is not missed. Without a
    // watchable socket (no inotify, abstract or TCP address) a failed connect is final.
    let mut watch = crate::sysbus::session_socket()
        .as_deref()
        .and_then(crate::sysbus::SocketWatch::new);
    if let Some(w) = watch.as_mut() {
        w.stop_on(&stop);
    }
    let mut generation = 0;
    let mut served = false;
    loop {
        match serve_on(&conn_out, &fetch, &tx, &tick, &ticks, generation) {
            End::Stopped => return,
            // A closed connection ends its subscriptions, the fetch loop's with them: the tray
            // serves again on a new one at once, as the shared connection reconnects on its
            // next use (a socket-activated bus keeps its socket, so no event marks its return).
            End::Closed => {
                generation += 1;
                served = true;
            }
            // No bus to connect to: the items of the lost connection are gone with it, and the
            // tray serves again on the bus socket's next event (created, moved in or touched
            // after listen(2)), never on a timer.
            End::NoBus => {
                if std::mem::take(&mut served) && tx.send(vec![]).is_err() {
                    return;
                }
                if !watch.as_mut().is_some_and(crate::sysbus::SocketWatch::wait) {
                    return;
                }
            }
        }
    }
}

/// Sends [`Ev::Closed`] once the router drops the fetch loop's sink, which it does when its
/// connection closes (or the tray cancels it, after its [`Ev::Stop`]).
struct Ended(Sender<Ev>, u64);

impl Drop for Ended {
    fn drop(&mut self) {
        let _ = self.0.send(Ev::Closed(self.1));
    }
}

/// The tray on one connection, until the tray stops or the connection closes.
fn serve_on(
    conn_out: &Mutex<Option<Connection>>,
    fetch: &Mutex<FetchSub>,
    tx: &crate::svc::Sender<Vec<Snap>>,
    tick: &Sender<Ev>,
    ticks: &Receiver<Ev>,
    generation: u64,
) -> End {
    let Some(list) = crate::dbus::session().and_then(|c| watcher_items(&c)) else {
        elog::eprintln!("sni: session bus unavailable");
        return End::NoBus;
    };
    // Fetch tasks report into the same queue.
    let done = tick.clone();
    // Signals go straight from the router into the fetch loop's queue, in bus order, so a
    // `dbus::sync` after a reply finds every signal that preceded it queued.
    let sink: crate::dbus::Sink = {
        let tick = tick.clone();
        let ended = Ended(tick.clone(), generation);
        Box::new(move |m| {
            let _ = &ended;
            let h = m.header();
            if h.member().is_some_and(|n| n.as_str() == "NameOwnerChanged") {
                let Ok((name, _, new)) = m.body().deserialize::<(String, String, String)>() else {
                    return true;
                };
                return tick.send(Ev::Owner { name, new }).is_ok();
            }
            let ev = match (h.interface().map(|i| i.as_str()), h.sender(), h.path()) {
                (Some(ITEM), Some(sender), Some(path)) => {
                    let member = h.member().map_or(String::new(), |n| n.to_string());
                    let status = (member == "NewStatus")
                        .then(|| m.body().deserialize::<String>().ok())
                        .flatten();
                    Ev::Item {
                        sender: sender.to_string(),
                        path: path.to_string(),
                        member,
                        status,
                    }
                }
                _ => Ev::List,
            };
            tick.send(ev).is_ok()
        })
    };
    let Some((conn, cancel)) = rules()
        .map_err(|e| elog::eprintln!("sni: rules: {e}"))
        .ok()
        .and_then(|r| crate::dbus::subscribe_to_cancellable(crate::dbus::Bus::Session, r, sink))
    else {
        elog::eprintln!("sni: session bus unavailable");
        return End::NoBus;
    };
    {
        let mut f = fetch.lock().unwrap_or_else(|e| e.into_inner());
        if f.stopped {
            drop(f);
            cancel.cancel();
            return End::Stopped;
        }
        // The previous connection's has ended with it.
        f.cancel = Some(cancel);
    }
    *conn_out.lock().unwrap_or_else(|e| e.into_inner()) = Some(conn.clone());
    let mut own = claim_watcher(&conn, &list);
    // `org.freedesktop.StatusNotifierHost-PID-ID` in the spec's words, one ID per tray.
    static HOST_IDS: AtomicU64 = AtomicU64::new(1);
    let host = format!(
        "org.kde.StatusNotifierHost-{}-{}",
        std::process::id(),
        HOST_IDS.fetch_add(1, Ordering::Relaxed)
    );
    // Released when the tray's loop ends: the connection, and the Watcher it serves, live on.
    let _host_name = conn
        .request_name(host.as_str())
        .is_ok()
        .then(|| OwnedName(conn.clone(), host.clone()));
    register_host(&conn, &host);
    let c = conn;
    let me = c.unique_name().map(|n| n.to_string());
    let watched = list;
    // The Watcher name's owner the host last registered with, when known.
    let mut hosted = own.as_ref().and(me.clone());
    // Registered keys in registration order; fetched items by key with their owner's unique
    // name (what item signals come from).
    let mut keys: Vec<String> = Vec::new();
    let mut items: HashMap<String, (Snap, Option<String>)> = HashMap::new();
    let mut list = true;
    let mut dirty: HashMap<String, Dirty> = HashMap::new();
    let mut last: Option<Vec<Snap>> = None;
    // Registered keys whose GetAll failed, with the unique name that owned their bus name then:
    // asked again only once that item signals or the name changes owner, not on every batch.
    let mut failed: HashMap<String, Option<String>> = HashMap::new();
    // The one fetch in flight per key: its id (a later reply of a dropped fetch is stale) and
    // task (dropping it cancels the call). Nothing waits on a fetch: an item that does not
    // answer holds only its own, which its owner's exit (the bus's `NoReply`, then
    // `NameOwnerChanged`) ends.
    let mut fetching: HashMap<String, (u64, zbus::Task<()>)> = HashMap::new();
    // A foreign Watcher's listing in flight, likewise.
    let mut listing: Option<(u64, zbus::Task<()>)> = None;
    let mut ids = 0u64;
    loop {
        if list && listing.is_none() {
            list = false;
            match &own {
                Some(l) => {
                    keys = l.lock().unwrap().clone();
                    items.retain(|k, _| keys.contains(k));
                    failed.retain(|k, _| keys.contains(k));
                    fetching.retain(|k, _| keys.contains(k));
                }
                None => {
                    ids += 1;
                    let (id, ac, done) = (ids, c.inner().clone(), done.clone());
                    let t = task(&c, "sni RegisteredStatusNotifierItems", async move {
                        let keys = registered(&ac).await;
                        let _ = done.send(Ev::Listed { id, keys });
                    });
                    listing = Some((id, t));
                }
            }
        }
        dirty.retain(|k, _| items.contains_key(k) || fetching.contains_key(k));
        // New items, failed ones something has happened to since, and the changes signalled
        // for fetched ones, each read by a task of its own.
        for k in &keys {
            if fetching.contains_key(k) {
                continue;
            }
            let (known, props) = match items.get_mut(k) {
                None if failed.contains_key(k) => continue,
                None => (None, None),
                Some((s, o)) => {
                    let Some(d) = dirty.remove(k) else { continue };
                    if let Some(st) = &d.status {
                        s.status = status(st);
                    }
                    match (d.all, d.props.is_empty()) {
                        (true, _) => (None, None),
                        (false, false) => (o.clone(), Some(d.props)),
                        (false, true) => continue,
                    }
                }
            };
            ids += 1;
            let (id, key, ac, done) = (ids, k.clone(), c.inner().clone(), done.clone());
            let t = task(&c, "sni fetch", async move {
                let bus = split_key(&key).0;
                let owner = match known {
                    Some(o) => Some(o),
                    None if bus.starts_with(':') => Some(bus.to_string()),
                    // A name nobody owns has no item to ask.
                    None => owner_of(&ac, bus).await,
                };
                let got = match (&owner, props) {
                    (None, None) => Got::Item(None),
                    (None, Some(_)) => Got::Props(vec![]),
                    (Some(o), None) => Got::Item(fetch_item(&ac, o, &key).await.map(Box::new)),
                    (Some(o), Some(p)) => Got::Props(fetch_props(&ac, o, &key, &p).await),
                };
                let _ = done.send(Ev::Fetched {
                    key,
                    id,
                    owner,
                    got,
                });
            });
            fetching.insert(k.clone(), (id, t));
        }
        let snaps: Vec<Snap> = keys
            .iter()
            .filter_map(|k| items.get(k).map(|e| e.0.clone()))
            .collect();
        if last.as_ref() != Some(&snaps) {
            if tx.send(snaps.clone()).is_err() {
                return End::Stopped;
            }
            last = Some(snaps);
        }
        // Signals that arrive while fetches run are folded into one refetch per item once they
        // land: bursts coalesce by back-pressure.
        let Ok(first) = ticks.recv() else {
            return End::Stopped;
        };
        let mut batch: Vec<Ev> = std::iter::once(first).chain(ticks.try_iter()).collect();
        if batch
            .iter()
            .any(|e| matches!(e, Ev::Fetched { .. } | Ev::Listed { .. }))
        {
            // The round trip queues every signal the bus sent before those replies: they join
            // this batch, so one that a reply already covers starts no second fetch after it.
            crate::dbus::sync(crate::dbus::Bus::Session);
            batch.extend(ticks.try_iter());
        }
        // The Watcher name's latest owner in this batch, if it changed.
        let mut watcher_owner: Option<String> = None;
        for ev in batch {
            let (sender, path, member, status) = match ev {
                Ev::List => {
                    list = true;
                    continue;
                }
                Ev::Stop => return End::Stopped,
                Ev::Closed(g) if g == generation => return End::Closed,
                Ev::Closed(_) => continue,
                Ev::Listed { id, keys: k } => {
                    if listing.as_ref().is_some_and(|l| l.0 == id) {
                        listing = None;
                        keys = k;
                        items.retain(|k, _| keys.contains(k));
                        failed.retain(|k, _| keys.contains(k));
                        fetching.retain(|k, _| keys.contains(k));
                    }
                    continue;
                }
                Ev::Fetched {
                    key,
                    id,
                    owner,
                    got,
                } => {
                    if fetching.get(&key).is_none_or(|f| f.0 != id) {
                        continue;
                    }
                    fetching.remove(&key);
                    match got {
                        Got::Item(Some(s)) => {
                            failed.remove(&key);
                            icons_changed(&s);
                            items.insert(key, (*s, owner));
                        }
                        // A refetch that failed keeps what was read before.
                        Got::Item(None) => {
                            if !items.contains_key(&key) {
                                failed.insert(key, owner);
                            }
                        }
                        Got::Props(v) => {
                            if let Some((s, _)) = items.get_mut(&key) {
                                let icons = v.iter().any(|(name, _)| name.contains("Icon"));
                                for (name, var) in v {
                                    set_prop(s, name, var.as_ref());
                                }
                                if icons {
                                    icons_changed(s);
                                }
                            }
                        }
                    }
                    continue;
                }
                // A failed item's name has an owner other than the one asked, which may serve
                // it; a name losing its owner may unregister items. A fetch in flight may have
                // asked the previous owner: it is dropped, and an item read from another owner
                // is read again from the new one.
                Ev::Owner { name, new } => {
                    // A unique name gets its one owner when it connects: no fetch asked another.
                    // Its signal is routed apart from the Watcher's method calls, so it may come
                    // after that connection's registration started the first fetch of its item.
                    if name.starts_with(':') && new == name {
                        continue;
                    }
                    if name == WATCHER {
                        watcher_owner = Some(new.clone());
                        list = true;
                    }
                    let mut again: Vec<String> = vec![];
                    fetching.retain(|k, _| {
                        let hit = split_key(k).0 == name;
                        if hit {
                            again.push(k.clone());
                        }
                        !hit
                    });
                    if new.is_empty() {
                        list = true;
                    } else {
                        failed.retain(|k, o| {
                            split_key(k).0 != name || o.as_deref() == Some(new.as_str())
                        });
                        again.extend(
                            items
                                .iter()
                                .filter(|(k, (_, o))| {
                                    split_key(k).0 == name && o.as_deref() != Some(new.as_str())
                                })
                                .map(|(k, _)| k.clone()),
                        );
                    }
                    for k in again {
                        if items.contains_key(&k) {
                            dirty.entry(k).or_default().all = true;
                        }
                    }
                    continue;
                }
                Ev::Item {
                    sender,
                    path,
                    member,
                    status,
                } => (sender, path, member, status),
            };
            // A failed item that signals is up again: fetch it whole on the next pass.
            failed
                .retain(|k, o| split_key(k).1 != path || o.as_ref().is_some_and(|o| *o != sender));
            // Only the item that signalled; an unknown owner matches by path alone. A
            // well-known name may have changed owner since: resolve it again.
            let mut hit: Vec<String> = items
                .iter()
                .filter(|(k, (_, o))| {
                    split_key(k).1 == path && o.as_ref().is_none_or(|o| *o == sender)
                })
                .map(|(k, _)| k.clone())
                .collect();
            if hit.is_empty() {
                for (k, (_, o)) in items.iter_mut() {
                    if split_key(k).1 == path && !k.starts_with(':') {
                        *o = name_owner(&c, split_key(k).0);
                        if o.as_ref().is_none_or(|o| *o == sender) {
                            hit.push(k.clone());
                        }
                    }
                }
            }
            // An item whose first GetAll is in flight: the signal may postdate its reply.
            hit.extend(
                fetching
                    .keys()
                    .filter(|k| {
                        !items.contains_key(*k)
                            && split_key(k).1 == path
                            && (!k.starts_with(':') || split_key(k).0 == sender)
                    })
                    .cloned(),
            );
            for k in hit {
                let d = dirty.entry(k).or_default();
                match (signal_props(&member), &status) {
                    (Some(["Status"]), Some(st)) => d.status = Some(st.clone()),
                    (Some(props), _) => {
                        for p in props {
                            if !d.props.contains(p) {
                                d.props.push(p);
                            }
                        }
                        if member == "NewStatus" {
                            d.status = None;
                        }
                    }
                    (None, _) => d.all = true,
                }
            }
        }
        // Like Quickshell's watcher and host: a Watcher name nobody owns any more is claimed
        // (an earlier owner's items re-register with whoever owns it next), and the host
        // registers with every new owner, this tray's own Watcher included. A listing asked of
        // an earlier owner is dropped.
        if watcher_owner.is_some() {
            listing = None;
        }
        match watcher_owner {
            Some(new) if new.is_empty() => {
                hosted = None;
                own = claim_watcher(&c, &watched);
            }
            Some(new) => {
                own = (me.as_deref() == Some(new.as_str())).then(|| watched.clone());
                if hosted.as_ref() != Some(&new) {
                    register_host(&c, &host);
                    hosted = Some(new);
                }
            }
            None => {}
        }
    }
}

/// What `s` shows. Every icon is optional (StatusNotifierItem spec): while the item needs
/// attention its attention icon shows if it has one, else its normal icon; names come before
/// pixmaps, as in Quickshell. The overlay (name, else pixmaps) is painted over either.
fn look(s: &Snap) -> Look {
    let named = |n: &str| {
        Src::Named(if s.theme_path.is_empty() {
            format!("image://icon/{n}")
        } else {
            format!("image://icon/{n}?path={}", s.theme_path)
        })
    };
    let base = if s.status == 2 && !s.attention_name.is_empty() {
        named(&s.attention_name)
    } else if s.status == 2 && !s.attention_pixmaps.is_empty() {
        Src::Pixmaps(s.attention_pixmaps.clone())
    } else if !s.icon_name.is_empty() {
        named(&s.icon_name)
    } else {
        Src::Pixmaps(s.pixmaps.clone())
    };
    let overlay = if !s.overlay_name.is_empty() {
        Some(named(&s.overlay_name))
    } else if !s.overlay_pixmaps.is_empty() {
        Some(Src::Pixmaps(s.overlay_pixmaps.clone()))
    } else {
        None
    };
    Look { base, overlay }
}

/// A named icon without an overlay is the icon provider's own source; anything else is drawn
/// by the tray's `image://service.sni/` provider ([`icon`]).
fn icon_url(s: &Snap, pixgen: u64) -> String {
    match look(s) {
        Look {
            base: Src::Named(url),
            overlay: None,
        } => url,
        _ => format!("image://service.sni/{}/{pixgen}", s.key),
    }
}

/// Quickshell `closestPixmap`: the smallest pixmap covering `size`, else the largest.
fn closest(px: &[Pix], w: i32, h: i32) -> Option<&Pix> {
    let mut ret: Option<&Pix> = None;
    for p in px {
        let Some(r) = ret else {
            ret = Some(p);
            continue;
        };
        let ea = r.w >= w && r.h >= h;
        let na = p.w >= w && p.h >= h;
        let smaller = p.w < r.w || p.h < r.h;
        if (ea && na && smaller) || (!ea && !smaller) {
            ret = Some(p);
        }
    }
    ret
}

/// One layer of an `image://service.sni/...` source.
#[derive(Debug, PartialEq)]
pub enum Layer {
    /// Straight (not premultiplied) RGBA pixels at their native size, as they came: Quickshell
    /// scales them before they are a QPixmap (`DBusSniIconPixmap::createImage` is
    /// Format_ARGB32).
    Pixels(usize, usize, Vec<[u8; 4]>),
    /// An `image://icon/` source to draw.
    Icon(String),
}

/// An `image://service.sni/...` source: `base` (`None`: the item has no icon to show) with
/// `overlay` painted over it.
#[derive(Debug, PartialEq)]
pub struct TrayIcon {
    pub base: Option<Layer>,
    pub overlay: Option<Layer>,
}

/// The layers of an `image://service.sni/...` source requested at `w`x`h` (pixmaps: the one
/// closest to that size, Quickshell `closestPixmap`).
pub fn icon(src: &str, w: i32, h: i32) -> Option<TrayIcon> {
    let rest = src.strip_prefix("image://service.sni/")?;
    let (key, _) = rest.rsplit_once('/')?;
    let look = PIXMAPS.lock().unwrap().as_ref()?.get(key)?.clone();
    let (w, h) = if w > 0 && h > 0 { (w, h) } else { (100, 100) };
    let layer = |src: Src| match src {
        Src::Named(url) => Some(Layer::Icon(url)),
        Src::Pixmaps(px) => {
            let (w, h, px) = straight(closest(&px, w, h)?)?;
            Some(Layer::Pixels(w, h, px))
        }
    };
    Some(TrayIcon {
        base: layer(look.base),
        overlay: look.overlay.and_then(layer),
    })
}

/// Premultiplied RGBA pixels of an `image://service.sni/...` source's base pixmap at its native
/// size.
pub fn pixmap(src: &str, w: i32, h: i32) -> Option<(usize, usize, Vec<[u8; 4]>)> {
    match icon(src, w, h)?.base? {
        Layer::Pixels(w, h, px) => {
            let pm =
                |[r, g, b, a]: [u8; 4]| [r, g, b].map(|x| mul255::qt(x as u32, a as u32) as u8);
            let px = px.into_iter().map(|c| {
                let [r, g, b] = pm(c);
                [r, g, b, c[3]]
            });
            Some((w, h, px.collect()))
        }
        Layer::Icon(_) => None,
    }
}

/// The pixels of a pixmap as RGBA (D-Bus sends ARGB in network order), not premultiplied.
fn straight(p: &Pix) -> Option<(usize, usize, Vec<[u8; 4]>)> {
    let px = p.data.get(..pix_len(p)?)?.as_chunks::<4>().0;
    let px = px.iter().map(|c| [c[1], c[2], c[3], c[0]]).collect();
    Some((p.w as usize, p.h as usize, px))
}

/// Quickshell `DBusMenuItem::text()` (`mCleanLabel`): every `_` but the last character is
/// dropped, and the character after a dropped one is kept as is (`__x` -> `_x`).
fn menu_label(l: &str) -> String {
    let mut t: Vec<char> = l.chars().collect();
    let mut i = 0;
    while i + 1 < t.len() {
        if t[i] == '_' {
            t.remove(i);
        }
        i += 1;
    }
    t.into_iter().collect()
}

/// `icon-data` PNGs by `image://dbusmenu/<hash>` source (Quickshell `DBusMenuPngImage`). Every
/// menu fetch re-registers its icons, so the least recently fetched ones (menus long closed,
/// icons an app has since redrawn) are evicted beyond the budget.
static MENU_PNGS: Mutex<Option<crate::bytelru::ByteLru<String, Arc<Vec<u8>>>>> = Mutex::new(None);

const MENU_PNGS_BYTES: usize = 4 << 20;

fn menu_png_url(data: Vec<u8>) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    data.hash(&mut h);
    let key = format!("{:016x}", h.finish());
    let url = format!("image://dbusmenu/{key}");
    let mut pngs = MENU_PNGS.lock().unwrap();
    let pngs = pngs.get_or_insert_with(|| crate::bytelru::ByteLru::new(MENU_PNGS_BYTES));
    if pngs.get(&key).is_none() {
        let n = data.len();
        pngs.insert(key, Arc::new(data), n);
    }
    url
}

/// PNG bytes of an `image://dbusmenu/...` menu entry icon.
pub fn menu_png(src: &str) -> Option<Arc<Vec<u8>>> {
    let key = src.strip_prefix("image://dbusmenu/")?;
    MENU_PNGS.lock().unwrap().as_mut()?.get(key).cloned()
}

fn unwrap_v<'a>(v: &'a ZV<'a>) -> &'a ZV<'a> {
    match v {
        ZV::Value(b) => unwrap_v(b),
        v => v,
    }
}

fn menu_entry(id: i32, m: &HashMap<String, OwnedValue>) -> MEntry {
    let st = |k: &str| match m.get(k).map(|v| unwrap_v(v)) {
        Some(ZV::Str(s)) => Some(s.to_string()),
        _ => None,
    };
    let b = |k: &str| match m.get(k).map(|v| unwrap_v(v)) {
        Some(ZV::Bool(b)) => Some(*b),
        _ => None,
    };
    MEntry {
        id,
        separator: st("type").is_some_and(|t| t == "separator"),
        enabled: b("enabled").unwrap_or(true),
        visible: b("visible").unwrap_or(true),
        text: st("label").map(|l| menu_label(&l)).unwrap_or_default(),
        icon: match st("icon-name").filter(|n| !n.is_empty()) {
            Some(n) => format!("image://icon/{n}"),
            None => match m.get("icon-data").map(|v| unwrap_v(v)) {
                Some(ZV::Array(a)) if !a.is_empty() => menu_png_url(
                    a.iter()
                        .filter_map(|b| if let ZV::U8(x) = b { Some(*x) } else { None })
                        .collect(),
                ),
                _ => String::new(),
            },
        },
        button: match st("toggle-type").as_deref() {
            Some("checkmark") => 1,
            Some("radio") => 2,
            _ => 0,
        },
        check: match m.get("toggle-state").map(|v| unwrap_v(v)) {
            Some(ZV::I32(0)) | None => 0,
            Some(ZV::I32(1)) => 2,
            Some(_) => 1,
        },
        children: st("children-display").is_some_and(|c| c == "submenu"),
    }
}

/// A com.canonical.dbusmenu layout node: (id, properties, children).
type MenuLayout = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);

/// GetLayout(id, depth 1) + the children's properties, after `AboutToShow(id)` when the
/// level is about to be shown (its first opener): the spec's "the menu is about to be shown",
/// which apps answer by rebuilding it (often with a `LayoutUpdated`), so a refetch never sends
/// it again.
///
/// Each call waits `timeout` for the app at most, and ends at once when `unwanted` does (the
/// fetch is dropped): a menu's lane is never held by an app that does not answer. `None` then,
/// and when the layout cannot be had.
fn fetch_menu(
    conn: &Connection,
    (svc, path, id): (&str, &str, i32),
    about_to_show: bool,
    timeout: std::time::Duration,
    unwanted: &Unwanted,
) -> Option<Level> {
    // `None`: the fetch is unwanted.
    fn call<B>(
        conn: &Connection,
        (svc, path): (&str, &str),
        timeout: std::time::Duration,
        unwanted: &Unwanted,
        method: &str,
        body: &B,
    ) -> Option<zbus::Result<zbus::Message>>
    where
        B: serde::ser::Serialize + zbus::zvariant::DynamicType,
    {
        crate::dbus::call_until(
            conn,
            timeout,
            // Ends when the last `Wanted` is dropped: nothing is ever sent.
            async {
                let _ = unwanted.recv().await;
            },
            Some(svc),
            path,
            Some(MENU),
            method,
            body,
        )
    }
    if about_to_show {
        // `DBusMenu::prepareToShow`: the layout is asked for whatever the answer, an error
        // or none in time included ("Error in AboutToShow, but showing anyway").
        call(conn, (svc, path), timeout, unwanted, "AboutToShow", &(id,))?
            .map_err(|e| {
                if crate::log_on!("ri::sni") {
                    tracing::debug!(target: "ri::sni", "sni: AboutToShow {svc}{path} {id}: {e}");
                }
            })
            .ok();
    }
    let r = call(
        conn,
        (svc, path),
        timeout,
        unwanted,
        "GetLayout",
        &(id, 1i32, Vec::<String>::new()),
    )?
    .map_err(|e| elog::eprintln!("sni: GetLayout {svc}{path} {id}: {e}"))
    .ok()?;
    let layout_at = r.recv_position();
    let (_, (_, _, kids)): (u32, MenuLayout) = r
        .body()
        .deserialize()
        .map_err(|e| elog::eprintln!("sni: GetLayout body: {e}"))
        .ok()?;
    let ids: Vec<i32> = kids
        .iter()
        .filter_map(|k| match unwrap_v(k) {
            ZV::Structure(st) => match st.fields().first() {
                Some(ZV::I32(i)) => Some(*i),
                _ => None,
            },
            _ => None,
        })
        .collect();
    if ids.is_empty() {
        return Some(Level {
            entries: vec![],
            layout_at,
            props_at: layout_at,
        });
    }
    let r = call(
        conn,
        (svc, path),
        timeout,
        unwanted,
        "GetGroupProperties",
        &(ids.clone(), Vec::<String>::new()),
    )?
    .map_err(|e| elog::eprintln!("sni: GetGroupProperties {svc}{path} {id}: {e}"))
    .ok()?;
    let props_at = r.recv_position();
    let mut props: HashMap<i32, HashMap<String, OwnedValue>> = r
        .body()
        .deserialize::<Vec<(i32, HashMap<String, OwnedValue>)>>()
        .ok()?
        .into_iter()
        .collect();
    Some(Level {
        entries: ids
            .iter()
            .map(|i| (*i, props.remove(i).unwrap_or_default()))
            .collect(),
        layout_at,
        props_at,
    })
}

fn str_of(w: &mut World, i: usize, k: &str) -> String {
    match w.read(i, k) {
        Some(Value::Str(s)) => s.to_string(),
        _ => String::new(),
    }
}

fn entry_fields(e: &MEntry) -> [(&'static str, &'static str, Value); 7] {
    [
        ("isSeparator", "bool", Value::Bool(e.separator)),
        ("enabled", "bool", Value::Bool(e.enabled)),
        ("text", "string", Value::str(&e.text)),
        ("icon", "string", Value::str(&e.icon)),
        ("buttonType", "int", Value::Num(e.button as f64)),
        ("checkState", "int", Value::Num(e.check as f64)),
        ("hasChildren", "bool", Value::Bool(e.children)),
    ]
}

fn new_entry(w: &mut World, svc: &str, path: &str, e: &MEntry) -> usize {
    let i = crate::types::native_singleton(w, "QsMenuEntry");
    for (k, t, v) in entry_fields(e).into_iter().chain([
        ("_svc", "string", Value::str(svc)),
        ("_path", "string", Value::str(path)),
        ("_id", "real", Value::Num(e.id as f64)),
    ]) {
        w.declare(i, k, t);
        w.write(i, k, v);
    }
    i
}

/// Rewrites a shown entry's properties in place (its item, and so the delegate's state, kept).
fn update_entry(w: &mut World, s: &mut Shown, e: MEntry) {
    if s.entry == e {
        return;
    }
    for (k, _, v) in entry_fields(&e) {
        w.write(s.item, k, v);
    }
    s.entry = e;
}

fn apply(w: &mut World, i: usize, s: &Snap, pixgen: u64) {
    if !s.menu.is_empty() && s.menu != "/NO_DBUSMENU" {
        let h = match w.read(i, "menu") {
            Some(Value::Item(h)) => h,
            _ => {
                let h = crate::types::native_singleton(w, "QsMenuHandle");
                w.declare(h, "_svc", "string");
                w.declare(h, "_path", "string");
                w.declare(h, "_id", "real");
                // The item owns its menu's handle: it goes with the item.
                w.owned.entry(i).or_default().push(h);
                w.write(i, "menu", Value::Item(h));
                h
            }
        };
        // The owner the item was read from: a menu is never asked of another.
        let svc = match s.owner.as_str() {
            "" => split_key(&s.key).0,
            o => o,
        };
        w.write(h, "_svc", Value::str(svc));
        w.write(h, "_path", Value::str(&s.menu));
    } else if let Some(Value::Item(h)) = w.read(i, "menu") {
        w.write(i, "menu", Value::Null);
        if let Some(o) = w.owned.get_mut(&i) {
            o.retain(|&x| x != h);
        }
        w.destroy(h);
    } else {
        w.write(i, "menu", Value::Null);
    }
    let fields: Vec<(&str, Value)> = vec![
        ("id", Value::str(&s.id)),
        ("title", Value::str(&s.title)),
        ("status", Value::Num(s.status as f64)),
        ("category", Value::Num(s.category as f64)),
        ("icon", Value::str(&icon_url(s, pixgen))),
        ("tooltipTitle", Value::str(&s.tip_title)),
        ("tooltipDescription", Value::str(&s.tip_desc)),
        (
            "hasMenu",
            Value::Bool(!s.menu.is_empty() && s.menu != "/NO_DBUSMENU"),
        ),
        ("onlyMenu", Value::Bool(s.only_menu)),
    ];
    for (k, v) in fields {
        w.write(i, k, v);
    }
}

impl Runtime {
    pub(crate) fn poll_sni(&mut self, calls: Vec<crate::svc::Call>) -> bool {
        let Some(&root) = self.world.borrow().singletons.get("SystemTray") else {
            return false;
        };
        let mut w = self.world.borrow_mut();
        if w.sni.is_none() {
            w.sni = start(&w.waker.handle(crate::svc::Svc::Sni));
            if w.sni.is_none() {
                return false;
            }
        }
        let mut m = w.sni.take().unwrap();
        for crate::svc::Call { method: k, args: a } in calls {
            let Some(Value::Item(it)) = a.first() else {
                continue;
            };
            if k == "MenuEvent" {
                let (svc, path) = (str_of(&mut w, *it, "_svc"), str_of(&mut w, *it, "_path"));
                let id = w.read(*it, "_id").map_or(0.0, |v| v.to_number()) as i32;
                if crate::log_on!("ri::sni") {
                    tracing::debug!(target: "ri::sni", "sni: menu event {svc}{path} {id}");
                }
                let Some(conn) = current(&m.conn) else {
                    continue;
                };
                // Refused its thread, the click is dropped: the entry is there to click again.
                on_lane(
                    &menu_lane(&mut m.menu_lanes, &format!("{svc}|{path}")),
                    "a click on an entry is dropped",
                    move || menu_event(&conn, &svc, &path, id, "clicked"),
                );
                continue;
            }
            let Some(key) = m.items.iter().find(|e| e.1 == *it).map(|e| e.0.clone()) else {
                continue;
            };
            let method = k;
            let delta = a.get(1).map_or(0.0, |v| v.to_number()) as i32;
            let horiz = a.get(2).is_some_and(|v| v.truthy());
            let Some(conn) = current(&m.conn) else {
                continue;
            };
            let c = conn.inner().clone();
            task(&conn, "sni item call", async move {
                let (bus, path) = split_key(&key);
                let r = match method.as_str() {
                    "Scroll" => {
                        c.call_method(
                            Some(bus),
                            path.as_str(),
                            Some(ITEM),
                            "Scroll",
                            &(delta, if horiz { "horizontal" } else { "vertical" }),
                        )
                        .await
                    }
                    _ => {
                        c.call_method(
                            Some(bus),
                            path.as_str(),
                            Some(ITEM),
                            method.as_str(),
                            &(0i32, 0i32),
                        )
                        .await
                    }
                };
                if let Err(e) = r
                    && !matches!(e, zbus::Error::MethodError(ref n, _, _) if n.as_str() == "org.freedesktop.DBus.Error.UnknownMethod")
                {
                    elog::eprintln!("sni {key} {method}: {e}");
                }
            })
            .detach();
        }
        let mut snap = None;
        while let Ok(s) = m.rx.try_recv() {
            snap = Some(s);
        }
        let mut changed = snap.is_some();
        changed |= poll_menus(&mut w, &mut m);
        if let Some(snaps) = snap {
            if crate::log_on!("ri::sni") {
                tracing::debug!(
                    target: "ri::sni",
                    "sni: {} items {:?}",
                    snaps.len(),
                    snaps
                        .iter()
                        .map(|s| (&s.id, s.status, s.pixmaps.len()))
                        .collect::<Vec<_>>()
                );
            }
            let old_order: Vec<String> = m.items.iter().map(|e| e.0.clone()).collect();
            let mut next = Vec::with_capacity(snaps.len());
            let mut pix = HashMap::new();
            for s in snaps {
                let (i, gen_, write) = match m.items.iter().position(|e| e.0 == s.key) {
                    Some(p) => {
                        let (_, i, old, g) = m.items.remove(p);
                        // A new URL whenever what is drawn changes: other pixmaps or names,
                        // or a status change between the normal and the attention icon (the
                        // scene caches a decoded source by URL).
                        let same = look(&old) == look(&s);
                        (
                            i,
                            if same {
                                g
                            } else {
                                m.pixgen += 1;
                                m.pixgen
                            },
                            old != s,
                        )
                    }
                    None => {
                        m.pixgen += 1;
                        (
                            crate::types::native_singleton(&mut w, "SystemTrayItem"),
                            m.pixgen,
                            true,
                        )
                    }
                };
                pix.insert(s.key.clone(), look(&s));
                if write {
                    apply(&mut w, i, &s, gen_);
                }
                next.push((s.key.clone(), i, s, gen_));
            }
            *PIXMAPS.lock().unwrap() = Some(pix);
            for (_, i, _, _) in std::mem::take(&mut m.items) {
                w.destroy(i);
            }
            m.items = tray_order(next, &old_order);
            let vals = m.items.iter().map(|e| Value::Item(e.1)).collect();
            if let Some(Value::Item(model)) = w.read(root, "items") {
                w.write(model, "values", Value::array(vals));
            }
        }
        w.sni = Some(m);
        changed
    }
}

/// Quickshell `SystemTray::compareItems` (sic: `||`, descending case-insensitive id).
fn tray_before(a: &Snap, b: &Snap) -> bool {
    a.category < b.category || a.id.to_lowercase() >= b.id.to_lowercase()
}

/// Known items keep their model position; new ones go through `ObjectModel::insertObjectSorted`
/// in registration order, so the order matches quickshell's.
fn tray_order<T>(
    next: Vec<(String, usize, Snap, T)>,
    old_order: &[String],
) -> Vec<(String, usize, Snap, T)> {
    let (mut out, fresh): (Vec<_>, Vec<_>) =
        next.into_iter().partition(|e| old_order.contains(&e.0));
    out.sort_by_key(|e| old_order.iter().position(|k| *k == e.0));
    for e in fresh {
        let at = out
            .iter()
            .position(|x| !tray_before(&e.2, &x.2))
            .unwrap_or(out.len());
        out.insert(at, e);
    }
    out
}

/// `DBusMenu::sendEvent`: `Event(id, name, <int32 0>, now)`, only written here, as Quickshell's
/// pending call is: nothing waits for the app, whose answer is logged when it is an error. An
/// app that never answers `Event` (it returns nothing) holds back neither its menu nor the
/// events after this one.
fn menu_event(conn: &Connection, svc: &str, path: &str, id: i32, ev: &str) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as u32);
    let what = format!("sni menu {svc}{path} {id} {ev}");
    let failed = what.clone();
    if let Err(e) = crate::dbus::call_then(
        conn,
        svc,
        path,
        MENU,
        "Event",
        &(id, ev, ZV::I32(0), ts),
        move |r| {
            if let Err(e) = r {
                elog::eprintln!("{failed}: {e}");
            }
        },
    ) {
        elog::eprintln!("{what}: {e}");
    }
}

/// Queues `f` on a menu's lane. `false`: the system refused the lane its thread (`EAGAIN`),
/// and `f` is dropped unrun. A menu call is never made here instead, on the main loop, where
/// an app that does not answer would stop the whole shell: the caller asks again at a later
/// pass. `then` says what that means, for the log.
fn on_lane(lane: &Arc<workpool::Lane>, then: &str, f: impl FnOnce() + Send + 'static) -> bool {
    match lane.try_run_shared(f) {
        Ok(()) => true,
        Err(e) => {
            static SAID: std::sync::Once = std::sync::Once::new();
            SAID.call_once(|| {
                elog::eprintln!("SystemTray: cannot start a menu's thread ({then}): {e}")
            });
            false
        }
    }
}

/// The service thread's connection now.
fn current(conn: &Mutex<Option<Connection>>) -> Option<Connection> {
    conn.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// The call queue of menu `svc|path`.
fn menu_lane(lanes: &mut HashMap<String, Arc<workpool::Lane>>, menu: &str) -> Arc<workpool::Lane> {
    lanes
        .entry(menu.to_string())
        .or_insert_with(|| Arc::new(workpool::Lane::new()))
        .clone()
}

fn split_menu_key(key: &str) -> (&str, &str, i32) {
    let mut p = key.splitn(3, '|');
    (
        p.next().unwrap_or(""),
        p.next().unwrap_or(""),
        p.next().and_then(|s| s.parse().ok()).unwrap_or(0),
    )
}

/// `svc|path|id` of the menu an opener shows, empty without one.
fn opener_key(w: &mut World, o: usize) -> String {
    match w.read(o, "menu") {
        Some(Value::Item(h)) => {
            let svc = str_of(w, h, "_svc");
            let path = str_of(w, h, "_path");
            let id = w.read(h, "_id").map_or(0.0, |v| v.to_number()) as i32;
            if svc.is_empty() || path.is_empty() {
                String::new()
            } else {
                format!("{svc}|{path}|{id}")
            }
        }
        _ => String::new(),
    }
}

/// Menu keys that gained / lost their first opener: `QsMenuEntry::ref`/`unref` emit
/// `opened`/`closed` on the refcount's 0 <-> 1 edges.
fn menu_edges(
    open: &std::collections::HashSet<String>,
    now: &std::collections::HashSet<String>,
) -> (Vec<String>, Vec<String>) {
    let mut opened: Vec<String> = now.difference(open).cloned().collect();
    let mut closed: Vec<String> = open.difference(now).cloned().collect();
    opened.sort();
    closed.sort();
    (opened, closed)
}

/// `svc|path` of a `svc|path|id` menu key.
fn menu_of(key: &str) -> &str {
    key.rsplit_once('|').map_or("", |r| r.0)
}

/// Whether `LayoutUpdated(_, parent)` covers the level of entry `id`: the whole layout, that
/// entry's own children, or a subtree above it. An entry whose ancestry was never fetched
/// counts as covered.
fn layout_covers(parents: Option<&HashMap<i32, i32>>, parent: i32, id: i32) -> bool {
    if parent == 0 || parent == id {
        return true;
    }
    let Some(parents) = parents else {
        return id != 0;
    };
    let mut cur = id;
    // At most one step per known entry: an app's id cycle ends the walk.
    for _ in 0..=parents.len() {
        if cur == 0 {
            return false;
        }
        match parents.get(&cur) {
            Some(&up) if up == parent => return true,
            Some(&up) => cur = up,
            None => return true,
        }
    }
    true
}

/// Writes the opener's children model: its visible entries, in layout order.
fn write_children(w: &mut World, o: usize, shown: &[Shown]) {
    let vis: Vec<Value> = shown
        .iter()
        .filter(|s| s.entry.visible)
        .map(|s| Value::Item(s.item))
        .collect();
    if let Some(Value::Item(model)) = w.read(o, "children") {
        w.write(model, "values", Value::array(vis));
    }
}

/// Shows a fetched level, reusing the items of entries it still has.
fn install(w: &mut World, o: usize, st: &mut Opener, level: Level) {
    let (svc, path, _) = split_menu_key(&st.key);
    let mut old: HashMap<i32, Shown> = std::mem::take(&mut st.shown)
        .into_iter()
        .map(|s| (s.entry.id, s))
        .collect();
    for (id, props) in level.entries {
        let e = menu_entry(id, &props);
        let s = match old.remove(&id) {
            Some(mut s) => {
                update_entry(w, &mut s, e);
                s.props = props;
                s
            }
            None => Shown {
                item: new_entry(w, svc, path, &e),
                entry: e,
                props,
            },
        };
        st.shown.push(s);
    }
    for (_, s) in old {
        w.destroy(s.item);
    }
    st.layout_at = Some(level.layout_at);
    st.props_at = Some(level.props_at);
    write_children(w, o, &st.shown);
}

/// `ItemsPropertiesUpdated` applied to the shown entries it names.
fn patch(
    w: &mut World,
    o: usize,
    st: &mut Opener,
    updated: &[(i32, HashMap<String, OwnedValue>)],
    removed: &[(i32, Vec<String>)],
) -> bool {
    let mut touched = vec![];
    for (id, props) in updated {
        if let Some(at) = st.shown.iter().position(|s| s.entry.id == *id) {
            for (k, v) in props {
                if let Ok(v) = v.try_clone() {
                    st.shown[at].props.insert(k.clone(), v);
                }
            }
            touched.push(at);
        }
    }
    for (id, names) in removed {
        if let Some(at) = st.shown.iter().position(|s| s.entry.id == *id) {
            for n in names {
                st.shown[at].props.remove(n);
            }
            touched.push(at);
        }
    }
    if touched.is_empty() {
        return false;
    }
    let mut visibility = false;
    for at in touched {
        let s = &mut st.shown[at];
        let e = menu_entry(s.entry.id, &s.props);
        visibility |= e.visible != s.entry.visible;
        update_entry(w, s, e);
    }
    if visibility {
        write_children(w, o, &st.shown);
    }
    true
}

/// A menu signal against what the opener shows: a `LayoutUpdated` newer than the shown layout
/// that covers its level marks it stale; an `ItemsPropertiesUpdated` newer than the shown
/// properties patches them. Older ones are already in the replies (the app answered after
/// sending them).
fn note(
    w: &mut World,
    o: usize,
    st: &mut Opener,
    parents: Option<&HashMap<i32, i32>>,
    at: Sequence,
    sig: &MenuSig,
) -> bool {
    match sig {
        MenuSig::Layout { parent } => {
            if st.layout_at.is_none_or(|l| at > l)
                && layout_covers(parents, *parent, split_menu_key(&st.key).2)
            {
                st.stale = true;
            }
            false
        }
        MenuSig::Props { updated, removed } => match st.props_at {
            Some(p) if at > p => patch(w, o, st, updated, removed),
            _ => false,
        },
    }
}

/// The signals of menu `menu` (`svc|path`): its sender, path and interface only, so another
/// app's (or another menu's) updates never reach this one.
fn menu_subscription(
    menu: &str,
    tx: crate::svc::Sender<MenuMsg>,
) -> Option<crate::dbus::Subscription> {
    let (svc, path) = menu.split_once('|')?;
    let rule = MatchRule::builder()
        .msg_type(MsgType::Signal)
        .sender(svc)
        .and_then(|b| b.path(path))
        .and_then(|b| b.interface(MENU))
        .map_err(|e| elog::eprintln!("sni: menu match {menu}: {e}"))
        .ok()?
        .build()
        .into_owned();
    let menu = menu.to_string();
    let sink: crate::dbus::Sink = Box::new(move |m| {
        let h = m.header();
        let sig = match h.member().map(|n| n.as_str()) {
            Some("LayoutUpdated") => MenuSig::Layout {
                // A malformed one invalidates the whole layout.
                parent: m.body().deserialize::<(u32, i32)>().map_or(0, |b| b.1),
            },
            Some("ItemsPropertiesUpdated") => {
                let Ok((updated, removed)) = m.body().deserialize::<(
                    Vec<(i32, HashMap<String, OwnedValue>)>,
                    Vec<(i32, Vec<String>)>,
                )>() else {
                    return true;
                };
                MenuSig::Props { updated, removed }
            }
            _ => return true,
        };
        tx.send(MenuMsg {
            menu: menu.clone(),
            at: m.recv_position(),
            sig: Arc::new(sig),
        })
        .is_ok()
    });
    let sub = crate::dbus::subscription(crate::dbus::Bus::Session, vec![rule], sink);
    if sub.is_none() {
        elog::eprintln!("sni: menu match: no session bus");
    }
    sub
}

/// The signal subscription of one shown menu, made on the menu's lane and ended there when this
/// is dropped, so the main loop never waits on the bus (AddMatch is a round trip, RemoveMatch a
/// write) and the menu's GetLayout, queued after it, is sent once the match is on the bus.
struct MenuSub {
    lane: Arc<workpool::Lane>,
    sub: Arc<Mutex<Option<crate::dbus::Subscription>>>,
}

impl MenuSub {
    /// `None`: the lane was refused its thread, and nothing is subscribed.
    fn new(
        lane: Arc<workpool::Lane>,
        menu: &str,
        tx: crate::svc::Sender<MenuMsg>,
    ) -> Option<MenuSub> {
        let sub = Arc::new(Mutex::new(None));
        let (s, menu) = (sub.clone(), menu.to_string());
        on_lane(&lane, "the menu is asked for later", move || {
            *s.lock().unwrap_or_else(|e| e.into_inner()) = menu_subscription(&menu, tx);
        })
        .then_some(MenuSub { lane, sub })
    }
}

impl Drop for MenuSub {
    fn drop(&mut self) {
        let s = self.sub.clone();
        // Refused its thread, the lane is idle and the subscription ends here, with `sub`:
        // RemoveMatch is written without a reply to wait for (`bus::remove_match`).
        let _ = self
            .lane
            .try_run_shared(move || drop(s.lock().unwrap_or_else(|e| e.into_inner()).take()));
    }
}

/// Keep every live `QsMenuOpener`'s `children` in sync with its handle's DBusMenu layout.
fn poll_menus(w: &mut World, m: &mut Sni) -> bool {
    let mut changed = false;
    // The service thread reconnected: the menus' subscriptions ended with the old connection,
    // and what is shown is fetched again on the new one.
    let now_on = current(&m.conn).and_then(|c| c.unique_name().map(|n| n.to_string()));
    if now_on != m.menu_conn {
        m.menu_conn = now_on;
        m.menu_subs.clear();
        m.menu_parents.clear();
        for st in m.menus.values_mut() {
            st.layout_at = None;
            st.props_at = None;
            st.held.clear();
            st.stale = !st.key.is_empty();
        }
    }
    while let Ok((o, key, fetch, level)) = m.mrx.try_recv() {
        if crate::log_on!("ri::sni") {
            tracing::debug!(target: "ri::sni", "sni: menu got opener {o} {key} {:?} entries", level.as_ref().map(|l| l.entries.len()));
        }
        let Some(st) = m.menus.get_mut(&o) else {
            continue;
        };
        // A reply of a fetch the opener dropped (with its key, even if it came back to that
        // key since) is stale.
        if st
            .fetching
            .as_ref()
            .is_none_or(|f| f.0 != key || f.1 != fetch)
        {
            continue;
        }
        st.fetching = None;
        let held = std::mem::take(&mut st.held);
        if st.key != key || w.dead.contains(&o) {
            continue;
        }
        let parents = m.menu_parents.entry(menu_of(&key).to_string()).or_default();
        match level {
            Some(level) => {
                let id = split_menu_key(&key).2;
                for (c, _) in &level.entries {
                    parents.insert(*c, id);
                }
                install(w, o, st, level);
            }
            None => {
                for s in std::mem::take(&mut st.shown) {
                    w.destroy(s.item);
                }
                st.layout_at = None;
                st.props_at = None;
                write_children(w, o, &[]);
            }
        }
        let parents = m.menu_parents.get(menu_of(&key));
        for (at, sig) in held {
            note(w, o, st, parents, at, &sig);
        }
        changed = true;
    }
    while let Ok(msg) = m.srx.try_recv() {
        let parents = m.menu_parents.get(&msg.menu);
        for (&o, st) in m.menus.iter_mut() {
            if st.key.is_empty() || menu_of(&st.key) != msg.menu {
                continue;
            }
            if st.fetching.as_ref().is_some_and(|f| f.0 == st.key) {
                st.held.push((msg.at, msg.sig.clone()));
            } else {
                changed |= note(w, o, st, parents, msg.at, &msg.sig);
            }
        }
    }
    let openers: Vec<usize> = w
        .of_type("QsMenuOpener")
        .into_iter()
        .filter(|o| !w.dead.contains(o))
        .collect();
    // An opener's `menu` changing (a closed popup, another item) polls the tray.
    for &o in &openers {
        w.mirror(o, crate::svc::Svc::Sni);
    }
    for (o, st) in std::mem::take(&mut m.menus) {
        if openers.contains(&o) {
            m.menus.insert(o, st);
        } else {
            for s in st.shown {
                w.destroy(s.item);
            }
        }
    }
    let keys: Vec<(usize, String)> = openers.iter().map(|&o| (o, opener_key(w, o))).collect();
    let now: std::collections::HashSet<String> = keys
        .iter()
        .filter(|k| !k.1.is_empty())
        .map(|k| k.1.clone())
        .collect();
    let (opened, closed) = menu_edges(&m.menus_open, &now);
    m.menus_open = now;
    m.menus_opening.extend(opened);
    for key in closed {
        // Closed before its `opened` went out: the app never heard of it.
        if m.menus_opening.remove(&key) {
            continue;
        }
        let Some(conn) = current(&m.conn) else {
            continue;
        };
        let announced = m.menus_announced.clone();
        let k = key.clone();
        let queued = on_lane(
            &menu_lane(&mut m.menu_lanes, menu_of(&key)),
            "the menu's `closed` goes out later",
            move || {
                // An `opened` that never went out (the layout failed to load) is not closed.
                if !announced
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&k)
                {
                    return;
                }
                let (svc, path, id) = split_menu_key(&k);
                menu_event(&conn, svc, path, id, "closed");
            },
        );
        if !queued {
            // Still open for the app: the next pass closes it, or finds it shown again.
            m.menus_open.insert(key);
        }
    }
    let shown: std::collections::HashSet<&str> = m.menus_open.iter().map(|k| menu_of(k)).collect();
    m.menu_subs.retain(|k, _| shown.contains(k.as_str()));
    m.menu_parents.retain(|k, _| shown.contains(k.as_str()));
    // A lane still draining is kept, so a menu shown again queues behind its earlier calls.
    m.menu_lanes
        .retain(|k, l| shown.contains(k.as_str()) || Arc::strong_count(l) > 1);
    // Subscribed on the menu's lane, ahead of its first GetLayout, so no update after its reply
    // is missed.
    for menu in shown {
        if !m.menu_subs.contains_key(menu) {
            let lane = menu_lane(&mut m.menu_lanes, menu);
            if let Some(sub) = MenuSub::new(lane, menu, m.stx.clone()) {
                m.menu_subs.insert(menu.to_string(), sub);
            }
        }
    }
    for (o, key) in keys {
        let st = m.menus.entry(o).or_default();
        if st.key != key {
            st.key = key.clone();
            st.layout_at = None;
            st.props_at = None;
            st.held.clear();
            // A fetch for the previous key (another item, or the item's previous owner) is
            // dropped: its call in flight ends, and its reply is never applied.
            st.fetching = None;
            st.stale = !key.is_empty();
            if !st.shown.is_empty() {
                for s in std::mem::take(&mut st.shown) {
                    w.destroy(s.item);
                }
                write_children(w, o, &[]);
                changed = true;
            }
        }
        // Without its subscription (refused its thread) a menu is not fetched yet.
        if key.is_empty()
            || st.fetching.is_some()
            || !st.stale
            || !m.menu_subs.contains_key(menu_of(&key))
        {
            continue;
        }
        let Some(conn) = current(&m.conn) else {
            continue;
        };
        m.menu_gen += 1;
        let fetch = m.menu_gen;
        if crate::log_on!("ri::sni") {
            tracing::debug!(target: "ri::sni", "sni: menu fetch opener {o} {key}");
        }
        let tx = m.mtx.clone();
        let first = m.menus_opening.remove(&key);
        let announced = m.menus_announced.clone();
        let (wanted, unwanted) = async_channel::bounded(1);
        let k = key.clone();
        let queued = on_lane(
            &menu_lane(&mut m.menu_lanes, menu_of(&key)),
            "the menu is asked for later",
            move || {
                let (svc, path, id) = split_menu_key(&k);
                let announce = || {
                    announced
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(k.clone())
                };
                // A submenu entry is its own menu and is ref'd at once; the root menu only once
                // its handle has loaded the layout, by this fetch or a later one while it is
                // open.
                if first && id != 0 && announce() {
                    menu_event(&conn, svc, path, id, "opened");
                }
                let level =
                    fetch_menu(&conn, (svc, path, id), first, MENU_REPLY_TIMEOUT, &unwanted);
                let opened = id == 0 && level.is_some() && announce();
                let _ = tx.send((o, k.clone(), fetch, level));
                if opened {
                    menu_event(&conn, svc, path, id, "opened");
                }
            },
        );
        if queued {
            st.fetching = Some((key, fetch, wanted));
            st.stale = false;
        } else if first {
            // The next pass fetches it, as newly opened.
            m.menus_opening.insert(key);
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    /// docs/quickshell-differences.md: a pixmap of 1024 a side is kept as it came, one of 1025
    /// is averaged down to 1024.
    #[test]
    fn a_pixmap_is_kept_up_to_1024_a_side() {
        // Opaque, blue all or nothing in turn: averaged, it is neither.
        let wire = |n: usize| {
            let px = (0..n * n).flat_map(|i| [255, 0, 0, ((i % n + i / n) % 2 * 255) as u8]);
            (n as i32, n as i32, crate::dbusvar::Bytes(px.collect()))
        };
        let arr = crate::dbusvar::Var::Pixmaps(vec![wire(1024), wire(1025)]);
        let got = pixmaps(Some(&arr));
        let crate::dbusvar::Var::Pixmaps(sent) = &arr else {
            unreachable!()
        };
        assert_eq!((got[0].w, got[0].h), (1024, 1024));
        assert!(got[0].data == sent[0].2.0, "1024x1024 is changed");
        assert_eq!((got[1].w, got[1].h), (1024, 1024));
        let blue: Vec<u8> = got[1].data.chunks(4).map(|p| p[3]).collect();
        assert!(
            blue.iter().any(|b| ![0, 255].contains(b)),
            "1025x1025 is not averaged"
        );
    }

    #[test]
    fn icon_pixmap_sizes_are_checked_against_their_data() {
        // 32768*32768*4 wrapped to 0 in i32, so an empty array passed as a 32768x32768 icon
        // and the scaler read past its pixels: any tray app could crash the shell.
        let wire = |w: i32, h: i32, n: usize| (w, h, crate::dbusvar::Bytes(vec![0x80u8; n]));
        let arr = crate::dbusvar::Var::Pixmaps(vec![
            wire(32768, 32768, 0),
            wire(65536, 65536, 16),
            wire(i32::MAX, 2, 0),
            wire(2048, 1, 2048 * 4),
            wire(2, 2, 15),
            wire(-2, -2, 16),
            wire(2, 2, 16),
            wire(1024, 1, 1024 * 4),
        ]);
        let got: Vec<(i32, i32)> = pixmaps(Some(&arr)).iter().map(|p| (p.w, p.h)).collect();
        // Oversized but complete pixmaps are kept, averaged down to MAX_PIX_SIDE.
        assert_eq!(got, [(1024, 1), (2, 2), (1024, 1)]);

        // A pixmap that skipped the wire check still never indexes past its data.
        let short = Pix {
            w: 32768,
            h: 32768,
            data: vec![],
        };
        let ok = Pix {
            w: 1,
            h: 1,
            data: vec![255, 10, 20, 30],
        };
        let only = |p: Pix| Look {
            base: Src::Pixmaps(Arc::new(vec![p])),
            overlay: None,
        };
        PIXMAPS.lock().unwrap().get_or_insert_default().extend([
            ("bad.test".to_string(), only(short)),
            ("ok.test".to_string(), only(ok)),
        ]);
        assert_eq!(pixmap("image://service.sni/bad.test/0", 0, 0), None);
        assert_eq!(
            pixmap("image://service.sni/ok.test/0", 0, 0),
            Some((1, 1, vec![[10, 20, 30, 255]]))
        );
    }

    #[test]
    fn menu_pngs_are_bounded_and_refetched_icons_kept() {
        let icon = |i: u32| {
            let mut d = vec![0u8; 64 << 10];
            d[..4].copy_from_slice(&i.to_le_bytes());
            d
        };
        let first = menu_png_url(icon(0));
        assert_eq!(menu_png(&first).as_deref(), Some(&icon(0)));
        let mut last = String::new();
        for i in 1..200 {
            // The first menu is fetched again now and then: its icon stays.
            if i % 16 == 0 {
                assert_eq!(menu_png_url(icon(0)), first);
            }
            last = menu_png_url(icon(i));
        }
        let (len, bytes) = {
            let m = MENU_PNGS.lock().unwrap();
            let m = m.as_ref().unwrap();
            (m.len(), m.bytes())
        };
        assert!(bytes <= MENU_PNGS_BYTES, "{bytes} bytes cached");
        assert!(len < 200, "{len} icons cached");
        assert_eq!(menu_png(&first).as_deref(), Some(&icon(0)));
        assert_eq!(menu_png(&last).as_deref(), Some(&icon(199)));
        assert_eq!(
            menu_png(&menu_png_url(icon(1)).replace("dbusmenu/", "dbusmenu/x")),
            None
        );
    }

    fn snap(id: &str, cat: &str) -> Snap {
        Snap {
            key: id.into(),
            id: id.into(),
            category: category(cat),
            ..Default::default()
        }
    }

    #[test]
    fn menu_text_is_quickshell_clean_label() {
        assert_eq!(menu_label("_Open file"), "Open file");
        assert_eq!(menu_label("__Open"), "_Open");
        assert_eq!(menu_label("Save _As"), "Save As");
        assert_eq!(menu_label("trailing_"), "trailing_");
        assert_eq!(menu_label("a_b_c"), "abc");
    }

    #[test]
    fn menu_icon_data_becomes_png_source() {
        let png: Vec<u8> = b"\x89PNG fake".to_vec();
        let mut m = HashMap::new();
        m.insert(
            "icon-data".to_string(),
            OwnedValue::try_from(ZV::from(png.clone())).unwrap(),
        );
        let e = menu_entry(3, &m);
        assert!(e.icon.starts_with("image://dbusmenu/"), "{}", e.icon);
        assert_eq!(menu_png(&e.icon).as_deref(), Some(&png));
        m.insert(
            "icon-name".to_string(),
            OwnedValue::try_from(ZV::from("folder")).unwrap(),
        );
        assert_eq!(menu_entry(3, &m).icon, "image://icon/folder");
        assert_eq!(menu_entry(3, &HashMap::new()).icon, "");
    }

    #[test]
    fn communications_sorts_after_application_status() {
        // Registration order zeta (Communications) then alpha (ApplicationStatus): Quickshell
        // keeps zeta first since 2 < 3.
        let next = vec![
            ("zeta".to_string(), 1, snap("zeta", "Communications"), ()),
            (
                "alpha".to_string(),
                2,
                snap("alpha", "ApplicationStatus"),
                (),
            ),
            ("hw".to_string(), 3, snap("hw", "Hardware"), ()),
        ];
        let ids: Vec<String> = tray_order(next, &[]).into_iter().map(|e| e.0).collect();
        assert_eq!(ids, ["zeta", "alpha", "hw"]);
        assert_eq!(category("SystemServices"), 1);
        assert_eq!(category("bogus"), 2);
    }

    #[test]
    fn needs_attention_without_an_attention_icon_shows_the_normal_icon() {
        // AttentionIcon* are optional (StatusNotifierItem spec): an item that only names its
        // icon keeps showing it while it needs attention.
        let s = Snap {
            key: "named.test".into(),
            status: 2,
            icon_name: "app".into(),
            ..Default::default()
        };
        assert_eq!(icon_url(&s, 1), "image://icon/app");
        let s = Snap {
            attention_name: "app-alert".into(),
            ..s
        };
        assert_eq!(icon_url(&s, 1), "image://icon/app-alert");
        // Pixmaps only: the normal ones while no attention pixmap is set.
        let px = |c: u8| {
            Arc::new(vec![Pix {
                w: 1,
                h: 1,
                data: vec![255, c, 0, 0],
            }])
        };
        let s = Snap {
            key: "pix.test".into(),
            status: 2,
            pixmaps: px(10),
            ..Default::default()
        };
        assert_eq!(look(&s).base, Src::Pixmaps(px(10)));
        let s = Snap {
            attention_pixmaps: px(20),
            ..s
        };
        assert_eq!(look(&s).base, Src::Pixmaps(px(20)));
    }

    #[test]
    fn an_overlay_is_drawn_over_the_icon() {
        let px = |c: u8| {
            Arc::new(vec![Pix {
                w: 1,
                h: 1,
                data: vec![255, c, 0, 0],
            }])
        };
        let named = Snap {
            key: "overlay-name.test".into(),
            status: 1,
            icon_name: "app".into(),
            theme_path: "/opt/app/icons".into(),
            overlay_name: "emblem".into(),
            ..Default::default()
        };
        let pixmaps = Snap {
            key: "overlay-pix.test".into(),
            status: 1,
            pixmaps: px(10),
            overlay_pixmaps: px(30),
            ..Default::default()
        };
        let plain = Snap {
            key: "plain.test".into(),
            icon_name: "app".into(),
            ..Default::default()
        };
        assert_eq!(icon_url(&plain, 1), "image://icon/app");
        let (a, b) = (icon_url(&named, 1), icon_url(&pixmaps, 1));
        assert_eq!(a, "image://service.sni/overlay-name.test/1");
        PIXMAPS.lock().unwrap().get_or_insert_default().extend([
            (named.key.clone(), look(&named)),
            (pixmaps.key.clone(), look(&pixmaps)),
        ]);
        assert_eq!(
            icon(&a, 0, 0),
            Some(TrayIcon {
                base: Some(Layer::Icon("image://icon/app?path=/opt/app/icons".into())),
                overlay: Some(Layer::Icon(
                    "image://icon/emblem?path=/opt/app/icons".into()
                )),
            })
        );
        assert_eq!(
            icon(&b, 0, 0),
            Some(TrayIcon {
                base: Some(Layer::Pixels(1, 1, vec![[10, 0, 0, 255]])),
                overlay: Some(Layer::Pixels(1, 1, vec![[30, 0, 0, 255]])),
            })
        );
    }

    #[test]
    fn menu_events_on_first_and_last_opener() {
        let set = |v: &[&str]| v.iter().map(|s| s.to_string()).collect();
        let (o, c) = menu_edges(&set(&[]), &set(&["a|/m|0"]));
        assert_eq!((o, c), (vec!["a|/m|0".to_string()], vec![]));
        let (o, c) = menu_edges(&set(&["a|/m|0"]), &set(&["a|/m|0", "a|/m|7"]));
        assert_eq!((o, c), (vec!["a|/m|7".to_string()], vec![]));
        let (o, c) = menu_edges(&set(&["a|/m|0", "a|/m|7"]), &set(&[]));
        assert_eq!(
            (o, c),
            (vec![], vec!["a|/m|0".to_string(), "a|/m|7".to_string()])
        );
        assert_eq!(split_menu_key("a|/m|7"), ("a", "/m", 7));
    }

    /// An app that answers no menu call, and a menu that stays open: `AboutToShow` is given
    /// up at the reply limit and the layout asked for all the same, as Quickshell does
    /// ("Error in AboutToShow, but showing anyway"); the unanswered `GetLayout` then ends the
    /// fetch at the limit, with no menu. A silent app sends nothing to wait on, so the limit
    /// is a clock (docs/clocks.md).
    #[test]
    fn a_silent_apps_menu_fetch_ends_at_the_reply_limit() {
        use std::io::BufRead;
        let dir = tempfile::tempdir().unwrap();
        let Ok(mut daemon) = std::process::Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
            .arg(format!(
                "--address=unix:path={}",
                dir.path().join("bus").display()
            ))
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
        else {
            crate::testenv::unavailable("no dbus-daemon");
            return;
        };
        let mut addr = String::new();
        std::io::BufReader::new(daemon.stdout.take().unwrap())
            .read_line(&mut addr)
            .unwrap();
        let peer = || {
            crate::dbus::builder(addr.trim())
                .and_then(|b| b.build())
                .expect("the private bus")
        };
        // A connection with no object server answers no call.
        let (c, silent) = (peer(), peer());
        let name = silent.unique_name().unwrap().to_string();
        let mut calls = zbus::blocking::MessageIterator::from(&silent).filter_map(|m| {
            let m = m.ok()?;
            (m.message_type() == MsgType::MethodCall)
                .then(|| m.header().member().unwrap().to_string())
        });
        let (_wanted, unwanted) = async_channel::bounded(1);
        let level = fetch_menu(
            &c,
            (&name, "/MenuBar", 0),
            true,
            std::time::Duration::from_millis(100),
            &unwanted,
        );
        assert!(level.is_none(), "a menu without an answer");
        assert_eq!(calls.next().as_deref(), Some("AboutToShow"));
        assert_eq!(calls.next().as_deref(), Some("GetLayout"));
        let _ = daemon.kill();
        let _ = daemon.wait();
    }

    /// The system refuses the tray's thread (the limit on threads is reached): `start`
    /// returns no tray, instead of panicking, and `poll_sni` starts it again at its next pass.
    /// The refusal is in the log, once. In a process of its own: it is said once per process,
    /// and the test reads what that process wrote to its stderr.
    #[test]
    fn a_refused_thread_leaves_no_tray() {
        const TEST: &str = concat!(module_path!(), "::a_refused_thread_leaves_no_tray");
        if !crate::testenv::in_own_process(TEST) {
            crate::testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
            return;
        }
        let waker = crate::svc::Waker::default();
        crate::testenv::refuse_new_threads();
        let (twice, log) = crate::testenv::stderr_with(|| {
            let h = waker.handle(crate::svc::Svc::Sni);
            [start(&h).is_none(), start(&h).is_none()]
        });
        assert_eq!(twice, [true, true]);
        assert_eq!(
            log.matches("SystemTray: cannot start its thread:").count(),
            1,
            "stderr: {log:?}"
        );
    }
}

#[cfg(test)]
mod menu_anchor_tests {
    use super::*;
    use crate::runtime::Runtime;

    fn entry(w: &mut World, id: i32, text: &str, children: bool) -> usize {
        let e = MEntry {
            id,
            enabled: true,
            visible: true,
            text: text.into(),
            children,
            ..Default::default()
        };
        new_entry(w, ":1.5", "/MenuBar", &e)
    }

    /// Live `QsMenuOpener`s and the item their `menu` points at.
    fn openers(rt: &Runtime) -> Vec<(usize, Option<usize>)> {
        let mut w = rt.world.borrow_mut();
        let os: Vec<usize> = w
            .of_type("QsMenuOpener")
            .into_iter()
            .filter(|o| !w.dead.contains(o))
            .collect();
        os.into_iter()
            .map(|o| match w.read(o, "menu") {
                Some(Value::Item(m)) => (o, Some(m)),
                _ => (o, None),
            })
            .collect()
    }

    /// QsMenuAnchor (waffle tray menus) is a popup of the anchor item's window whose levels
    /// cascade: activating an entry with children opens its level, a leaf triggers the
    /// DBusMenu item and closes the menu.
    #[test]
    fn menu_anchor_opens_cascades_and_triggers() {
        let d = std::env::temp_dir().join(format!("qml-menu-anchor-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("main.qml"),
            "import QtQuick\nimport Quickshell\nPanelWindow {\n id: win\n implicitWidth: 100; implicitHeight: 30\n\
             Item { id: btn; width: 20; height: 20 }\n QsMenuAnchor { id: m; anchor.item: btn; anchor.edges: Edges.Bottom }\n}\n",
        )
        .unwrap();
        let mut rt = Runtime::new(Some(d.clone()));
        rt.load_file(&d.join("main.qml")).unwrap();
        let _ = std::fs::remove_dir_all(&d);
        rt.settle();
        let (win, m, handle) = {
            let mut w = rt.world.borrow_mut();
            let win = w.of_type("PanelWindow")[0];
            let m = w.of_type("QsMenuAnchor")[0];
            assert!(
                w.items[m].types.iter().any(|t| t == "PopupWindow"),
                "{:?}",
                w.items[m].types
            );
            let h = crate::types::native_singleton(&mut w, "QsMenuHandle");
            for (k, v) in [
                ("_svc", Value::str(":1.5")),
                ("_path", Value::str("/MenuBar")),
                ("_id", Value::Num(0.0)),
            ] {
                w.declare(h, k, "var");
                w.write(h, k, v);
            }
            w.write(m, "menu", Value::Item(h));
            (win, m, h)
        };
        rt.settle();
        assert!(
            openers(&rt).iter().all(|o| o.1.is_none()),
            "a closed menu fetches nothing"
        );
        rt.call_method(m, "open", vec![]).unwrap().unwrap();
        rt.settle();
        {
            let mut w = rt.world.borrow_mut();
            assert!(w.read(m, "visible").unwrap().truthy());
            assert!(matches!(w.read(m, "anchor.window"), Some(Value::Item(x)) if x == win));
        }
        let top = openers(&rt);
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].1, Some(handle));
        let (leaf, sub) = {
            let mut w = rt.world.borrow_mut();
            let leaf = entry(&mut w, 1, "Quit", false);
            let sub = entry(&mut w, 2, "More", true);
            let model = match w.read(top[0].0, "children") {
                Some(Value::Item(c)) => c,
                _ => panic!("QsMenuOpener.children is not a model"),
            };
            w.write(
                model,
                "values",
                Value::array(vec![Value::Item(leaf), Value::Item(sub)]),
            );
            (leaf, sub)
        };
        rt.settle();
        rt.call_method(
            m,
            "activate",
            vec![Value::Num(0.0), Value::Item(sub), Value::Num(30.0)],
        )
        .unwrap()
        .unwrap();
        rt.settle();
        let ops = openers(&rt);
        assert_eq!(ops.len(), 2, "submenu level opened");
        assert!(ops.iter().any(|o| o.1 == Some(sub)));
        rt.world.borrow_mut().effects.clear();
        rt.call_method(
            m,
            "activate",
            vec![Value::Num(1.0), Value::Item(leaf), Value::Num(0.0)],
        )
        .unwrap()
        .unwrap();
        rt.settle();
        let mut w = rt.world.borrow_mut();
        assert!(
            !w.read(m, "visible").unwrap().truthy(),
            "a leaf closes the menu"
        );
        let ev = w
            .effects
            .iter()
            .filter(|e| matches!(e, crate::svc::Effect::Sni(c) if matches!(c.args.first(), Some(Value::Item(x)) if *x == leaf)))
            .count();
        assert_eq!(ev, 1, "triggered reaches DBusMenu once");
    }
}
