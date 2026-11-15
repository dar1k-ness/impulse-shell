//! Native `Rustshell.Services.Notifications`: a `NotificationServer` owning
//! `org.freedesktop.Notifications` (quickshell `src/services/notifications/`).
//!
//! The D-Bus thread queues each `Notify` request; `Runtime::poll_notifications` handles it in
//! order with the closes (updates a live `replaces_id` in place, else shows a new notification
//! under the non-zero `replaces_id` or the next free id), creates/updates the `Notification` items, emits
//! `NotificationServer.notification`, decides tracking and emits the close signals. Like
//! quickshell's `Notify` slot, the reply (the id) is sent only once that has run, so a flood's
//! senders wait for the shell instead of leaving it a backlog to work through after they finish.
use crate::mpris::{V, conv};
use crate::runtime::{Runtime, World};
use crate::value::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use zbus::blocking::Connection;

const NAME: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
const IFACE: &str = "org.freedesktop.Notifications";

/// `NotificationCloseReason`.
const EXPIRED: u32 = 1;
const DISMISSED: u32 = 2;
const CLOSE_REQUESTED: u32 = 3;
/// The spec's "undefined/reserved" reason: the server went away with the notification.
const SERVER_GONE: u32 = 4;

/// The object, the name and the takeover requests, changed in the order they were asked for: a
/// server stopped and started again (a Loader toggled) or a World replaced by the next one never
/// has an older change land after a newer one.
static BUS: workpool::Lane = workpool::Lane::new();
/// The instance ([`Notifs::instance`]) whose object is served and whose name is requested; 0 for
/// none. Read and written on [`BUS`] only.
static ACTIVE: AtomicU64 = AtomicU64::new(0);
static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(1);

/// quickshell `NotificationServerSupport` (note `body` defaults to true).
#[derive(Clone, PartialEq, Debug)]
pub struct Support {
    pub persistence: bool,
    pub body: bool,
    pub body_markup: bool,
    pub body_hyperlinks: bool,
    pub body_images: bool,
    pub actions: bool,
    pub action_icons: bool,
    pub image: bool,
    pub inline_reply: bool,
    pub extra_hints: Vec<String>,
}

impl Default for Support {
    fn default() -> Self {
        Support {
            persistence: false,
            body: true,
            body_markup: false,
            body_hyperlinks: false,
            body_images: false,
            actions: false,
            action_icons: false,
            image: false,
            inline_reply: false,
            extra_hints: vec![],
        }
    }
}

impl Support {
    /// quickshell `NotificationServer::GetCapabilities`.
    pub fn capabilities(&self) -> Vec<String> {
        let mut c = vec![];
        if self.persistence {
            c.push("persistence");
        }
        if self.body {
            c.push("body");
            if self.body_markup {
                c.push("body-markup");
            }
            if self.body_hyperlinks {
                c.push("body-hyperlinks");
            }
            if self.body_images {
                c.push("body-images");
            }
        }
        if self.actions {
            c.push("actions");
            if self.action_icons {
                c.push("action-icons");
            }
        }
        if self.image {
            c.push("icon-static");
        }
        if self.inline_reply {
            c.push("inline-reply");
        }
        let mut c: Vec<String> = c.into_iter().map(String::from).collect();
        c.extend(self.extra_hints.iter().cloned());
        c
    }
}

/// Premultiplied RGBA image decoded from an `image-data` hint.
#[derive(Clone, Default)]
pub struct Img {
    pub w: usize,
    pub h: usize,
    pub px: Vec<[u8; 4]>,
}

struct Req {
    /// `replaces_id`: honoured only if that notification is still live when this is handled.
    replaces: u32,
    app_name: String,
    app_icon: String,
    summary: String,
    body: String,
    actions: Vec<String>,
    hints: Vec<(String, V)>,
    /// `Some` when an image-data/image_data/icon_data hint was present (data may be empty).
    image: Option<Pending>,
    expire: i32,
    /// Gets the id once the request is handled: `Notify` replies then.
    done: Option<async_channel::Sender<u32>>,
}

/// An image hint being decoded on a pool worker, off the session bus executor; its request
/// waits at the head of the queue until it is done, so requests stay in the order they came.
struct Pending {
    rx: std::sync::mpsc::Receiver<Img>,
    img: Option<Img>,
}

impl Pending {
    fn decode(v: crate::dbusvar::Var, wake: crate::svc::Handle) -> Pending {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        workpool::run(move || {
            let _ = tx.send(image_hint(&v));
            wake.wake();
        });
        Pending { rx, img: None }
    }

    fn ready(&mut self) -> bool {
        if self.img.is_none() {
            match self.rx.try_recv() {
                Ok(img) => self.img = Some(img),
                Err(std::sync::mpsc::TryRecvError::Empty) => return false,
                // The decoder panicked: no image, like a hint that did not decode.
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.img = Some(Img::default()),
            }
        }
        true
    }
}

/// Whether `c` is a `Notify` waiting for its image hint to decode.
fn decoding(c: &mut Option<Cmd>) -> bool {
    if let Some(Cmd::Notify(r)) = c
        && let Some(p) = &mut r.image
    {
        !p.ready()
    } else {
        false
    }
}

enum Cmd {
    Notify(Box<Req>),
    /// `CloseNotification`; gets whether the notification was live.
    Close(u32, async_channel::Sender<bool>),
    /// The server is up on this connection.
    Ready(Connection),
    /// The name is requested and watched; the start job has nothing left to do.
    Watching,
}

struct Server {
    support: Arc<Mutex<Support>>,
    tx: Mutex<crate::svc::Sender<Cmd>>,
}

impl Server {
    fn send(&self, c: Cmd) {
        let _ = self.tx.lock().unwrap().send(c);
    }
}

/// `(iiibiiay)` → image. Like quickshell's `operator>>(DBusNotificationImage)`, a
/// mismatching sample/channel count only warns; rows honour the sent rowstride. Unlike it
/// (QImage over the sender's buffer), the sizes are checked against the data: a rowstride
/// shorter than a row or data shorter than `h` rows drops the image instead of reading past the
/// buffer. The bytes arrive typed (not one `Value` per byte). An image up to
/// [`crate::dbusvar::MAX_SIDE`] is kept as it came, as Quickshell keeps it (512x512 is shown
/// pixel for pixel: tests/tray_pixels_qt.rs in the shell); a larger one is kept averaged down,
/// so a huge image costs about its own size once while it is decoded and a bounded amount
/// while the notification lives.
fn image_hint(v: &crate::dbusvar::Var) -> Img {
    let empty = Img {
        w: 0,
        h: 0,
        px: vec![],
    };
    let crate::dbusvar::Var::Image(crate::dbusvar::ImageData {
        w,
        h,
        stride,
        alpha,
        bits,
        chans,
        data: d,
    }) = v
    else {
        return empty;
    };
    let need = if *alpha { 4 } else { 3 };
    if *bits != 8 || *chans != need || *stride as i64 != *w as i64 * need as i64 {
        elog::eprintln!(
            "notifications: image hint w={w} h={h} rowstride={stride} bits={bits} channels={chans} alpha={alpha}: unexpected layout"
        );
    }
    if d.is_empty() || *w <= 0 || *h <= 0 || *stride < 0 {
        return empty;
    }
    let (w, h, need) = (*w as usize, *h as usize, need as usize);
    let row = w * need;
    let stride = if *stride == 0 { row } else { *stride as usize };
    let fits = stride >= row
        && (h - 1)
            .checked_mul(stride)
            .and_then(|n| n.checked_add(row))
            .is_some_and(|n| n <= d.len());
    if !fits {
        elog::eprintln!(
            "notifications: image hint {w}x{h} rowstride={stride} needs more than its {} bytes; dropped",
            d.len()
        );
        return empty;
    }
    let (w, h, px) = crate::dbusvar::fit(w, h, crate::dbusvar::MAX_SIDE, |x, y| {
        let o = y * stride + x * need;
        let a = if need == 4 { d[o + 3] } else { 255 } as u32;
        let pm = |c: u8| mul255::qt(c as u32, a) as u8;
        [pm(d[o]), pm(d[o + 1]), pm(d[o + 2]), a as u8]
    });
    Img { w, h, px }
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Server {
    fn get_capabilities(&self) -> Vec<String> {
        self.support.lock().unwrap().capabilities()
    }

    #[allow(clippy::too_many_arguments)]
    async fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        mut hints: HashMap<String, crate::dbusvar::Var>,
        expire_timeout: i32,
    ) -> zbus::fdo::Result<u32> {
        // Decoding and downscaling walk every pixel: not on this executor, which serves the
        // whole shared session bus connection (MPRIS, replies, signals).
        let image = ["image-data", "image_data", "icon_data"]
            .iter()
            .find_map(|k| hints.remove(*k))
            .map(|v| Pending::decode(v, self.tx.lock().unwrap().handle().clone()));
        let mut hv: Vec<(String, V)> = hints
            .iter()
            .filter(|(k, _)| !matches!(k.as_str(), "image-data" | "image_data" | "icon_data"))
            .filter_map(|(k, v)| match v {
                crate::dbusvar::Var::Value(v) => Some((k.clone(), conv(v))),
                _ => None,
            })
            .collect();
        hv.sort_by(|a, b| a.0.cmp(&b.0));
        let (done, handled) = async_channel::bounded(1);
        self.send(Cmd::Notify(Box::new(Req {
            replaces: replaces_id,
            app_name,
            app_icon,
            summary,
            body,
            actions,
            hints: hv,
            image,
            expire: expire_timeout,
            done: Some(done),
        })));
        handled.recv().await.map_err(|_| {
            zbus::fdo::Error::Failed("the notification server stopped before showing it".into())
        })
    }

    async fn close_notification(&self, id: u32) -> zbus::fdo::Result<()> {
        let (done, handled) = async_channel::bounded(1);
        self.send(Cmd::Close(id, done));
        match handled.recv().await {
            Ok(true) => Ok(()),
            // Desktop Notifications 1.3: "If the notification no longer exists, an empty D-BUS
            // Error message is sent back."
            Ok(false) => Err(zbus::fdo::Error::Failed(String::new())),
            Err(_) => Err(zbus::fdo::Error::Failed(
                "the notification server stopped before closing it".into(),
            )),
        }
    }

    #[zbus(out_args("name", "vendor", "version", "spec_version"))]
    fn get_server_information(&self) -> (String, String, String, String) {
        // quickshell never sets QCoreApplication::applicationVersion, so `version` is empty.
        (
            "rustshell".into(),
            "rustshell".into(),
            String::new(),
            "1.2".into(),
        )
    }

    #[zbus(signal)]
    async fn notification_closed(
        em: &zbus::object_server::SignalEmitter<'_>,
        id: u32,
        reason: u32,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn action_invoked(
        em: &zbus::object_server::SignalEmitter<'_>,
        id: u32,
        action_key: &str,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn notification_replied(
        em: &zbus::object_server::SignalEmitter<'_>,
        id: u32,
        text: &str,
    ) -> zbus::Result<()>;
}

/// Decoded notification images by handle for `image://qsimage/<run>-<handle>/<change>` sources.
static IMAGES: Mutex<Option<HashMap<u32, Arc<Img>>>> = Mutex::new(None);
static NEXT_HANDLE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
/// Saved images kept on disk; older ones are pruned.
const SAVED_MAX: usize = 512;

/// This process's prefix of image handles, so a source persisted by an earlier run never names
/// a handle of this one.
fn run() -> &'static str {
    static RUN: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    RUN.get_or_init(|| {
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        format!("{ms:x}{:x}", std::process::id())
    })
}

fn saved_dir() -> std::path::PathBuf {
    crate::xdg::cache_home().join("impulse-shell/notification-images")
}

/// The on-disk copy of a source; `None` for anything not shaped like ours.
fn saved_path(src: &str) -> Option<std::path::PathBuf> {
    let (key, change) = src.strip_prefix("image://qsimage/")?.split_once('/')?;
    let (run, handle) = key.split_once('-')?;
    let ok = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric());
    (ok(run) && ok(handle) && ok(change))
        .then(|| saved_dir().join(format!("{run}-{handle}-{change}.png")))
}

/// Quickshell's `image://qsimage` only lives as long as the notification, but the original persists
/// `image` in notifications.json and shows it again after a restart; keep a copy for that.
/// Returns false when nothing was written.
fn write_png(path: &std::path::Path, img: &Img) -> bool {
    let mut rgba = Vec::with_capacity(img.px.len() * 4);
    for &[r, g, b, a] in &img.px {
        let un = |c: u8| match a {
            0 => 0,
            255 => c,
            _ => ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8,
        };
        rgba.extend_from_slice(&[un(r), un(g), un(b), a]);
    }
    let Some(buf) = image::RgbaImage::from_raw(img.w as u32, img.h as u32, rgba) else {
        return false;
    };
    // Written beside and renamed so a restarted shell never decodes a half-written PNG.
    let tmp = path.with_extension("tmp");
    if std::fs::create_dir_all(saved_dir()).is_err()
        || buf.save_with_format(&tmp, image::ImageFormat::Png).is_err()
        || std::fs::rename(&tmp, path).is_err()
    {
        let _ = std::fs::remove_file(&tmp);
        return false;
    }
    true
}

enum DiskOp {
    Save(std::path::PathBuf, Arc<Img>),
    Remove(std::path::PathBuf),
}

/// Disk work on the saved copies, done in the order it was asked for (a copy removed right
/// after it was saved stays removed) by one pool job at a time.
struct Disk {
    q: std::collections::VecDeque<DiskOp>,
    busy: bool,
}

static DISK: Mutex<Disk> = Mutex::new(Disk {
    q: std::collections::VecDeque::new(),
    busy: false,
});

/// Notified whenever a drain ends (`busy` cleared): what [`wait_disk_idle`] sleeps on.
static DISK_IDLE: std::sync::Condvar = std::sync::Condvar::new();

/// Saved copies, oldest first: listed from the directory once per process, then kept up to
/// date by the saves and removals themselves, so a save never lists or stats the directory.
#[derive(Default)]
struct Saved(std::collections::VecDeque<std::path::PathBuf>);

impl Saved {
    /// Records a written copy; returns the oldest ones past [`SAVED_MAX`], to delete.
    fn add(&mut self, path: std::path::PathBuf) -> Vec<std::path::PathBuf> {
        self.0.retain(|p| *p != path);
        self.0.push_back(path);
        let extra = self.0.len().saturating_sub(SAVED_MAX);
        self.0.drain(..extra).collect()
    }

    fn remove(&mut self, path: &std::path::Path) {
        self.0.retain(|p| p != path);
    }
}

static SAVED: Mutex<Option<Saved>> = Mutex::new(None);

fn disk(op: DiskOp) {
    let mut d = DISK.lock().unwrap_or_else(|e| e.into_inner());
    d.q.push_back(op);
    if !d.busy {
        d.busy = true;
        drop(d);
        workpool::run(drain_disk);
    }
}

/// Blocks until every save and removal of a saved copy queued so far is done.
pub fn wait_disk_idle() {
    let mut d = DISK.lock().unwrap_or_else(|e| e.into_inner());
    while d.busy || !d.q.is_empty() {
        d = DISK_IDLE.wait(d).unwrap_or_else(|e| e.into_inner());
    }
}

fn drain_disk() {
    /// Lets the next `disk` start a drain even if this one panicked.
    struct Idle;
    impl Drop for Idle {
        fn drop(&mut self) {
            DISK.lock().unwrap_or_else(|e| e.into_inner()).busy = false;
            DISK_IDLE.notify_all();
        }
    }
    let _idle = Idle;
    loop {
        let op = {
            let mut d = DISK.lock().unwrap_or_else(|e| e.into_inner());
            match d.q.pop_front() {
                Some(op) => op,
                None => {
                    // Cleared under the lock that `disk` checks, so no op is left behind.
                    d.busy = false;
                    return;
                }
            }
        };
        let mut saved = SAVED.lock().unwrap_or_else(|e| e.into_inner());
        let saved = saved.get_or_insert_with(list_saved);
        match op {
            DiskOp::Save(path, img) => {
                if write_png(&path, &img) {
                    for old in saved.add(path) {
                        let _ = std::fs::remove_file(old);
                    }
                }
            }
            DiskOp::Remove(path) => {
                let _ = std::fs::remove_file(&path);
                saved.remove(&path);
            }
        }
    }
}

/// The copies earlier runs left, oldest first, pruned to [`SAVED_MAX`].
fn list_saved() -> Saved {
    let Ok(rd) = std::fs::read_dir(saved_dir()) else {
        return Default::default();
    };
    let mut files: Vec<_> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "png"))
        .filter_map(|p| Some((std::fs::metadata(&p).ok()?.modified().ok()?, p)))
        .collect();
    files.sort();
    let extra = files.len().saturating_sub(SAVED_MAX);
    for (_, p) in files.drain(..extra) {
        let _ = std::fs::remove_file(p);
    }
    Saved(files.into_iter().map(|(_, p)| p).collect())
}

/// Deletes the saved copy behind `src`, if it names one: its notification is gone from the original's
/// history (dismissed, expired, closed, or replaced by a newer image).
pub(crate) fn forget_saved(src: &str) {
    if let Some(p) = saved_path(src) {
        disk(DiskOp::Remove(p));
    }
}

fn load_saved(src: &str) -> Option<(usize, usize, Vec<[u8; 4]>)> {
    let img = image::open(saved_path(src)?).ok()?.into_rgba8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    let px = img
        .pixels()
        .map(|p| {
            let [r, g, b, a] = p.0;
            let pm = |c: u8| mul255::qt(c as u32, a as u32) as u8;
            [pm(r), pm(g), pm(b), a]
        })
        .collect();
    Some((w, h, px))
}

/// Premultiplied RGBA pixels of an `image://qsimage/...` source at its native size.
pub fn pixmap(src: &str) -> Option<(usize, usize, Vec<[u8; 4]>)> {
    let rest = src.strip_prefix("image://qsimage/")?;
    let live = rest
        .split('/')
        .next()?
        .strip_prefix(run())
        .and_then(|h| h.strip_prefix('-')?.parse::<u32>().ok())
        .and_then(|h| IMAGES.lock().unwrap().as_ref()?.get(&h).cloned());
    match live {
        Some(img) => (img.w > 0 && img.h > 0).then(|| (img.w, img.h, img.px.clone())),
        None => load_saved(src).filter(|(w, h, _)| *w > 0 && *h > 0),
    }
}

struct Live {
    item: usize,
    actions: Vec<usize>,
    handle: u32,
    change: u32,
    hints: Vec<(String, V)>,
    /// The `image` source whose decoded pixels are saved on disk.
    saved: Option<String>,
}

pub struct Notifs {
    /// Tells this instance's start from a later one's (another World, a server made again).
    instance: u64,
    /// Set once the start job serves on it.
    conn: Option<Connection>,
    /// The name's owner watch (QDBusServiceWatcher), ended with this instance.
    watch: Arc<Mutex<Watch>>,
    rx: Receiver<Cmd>,
    /// Where the next free id is looked for (quickshell `NotificationServer::nextId`).
    next_id: u32,
    support: Arc<Mutex<Support>>,
    /// Tracked notifications in `trackedNotifications` order (ObjectModel::insertObject appends).
    live: BTreeMap<u32, Live>,
    order: Vec<u32>,
    /// `World::tracked_gen` at the last `tracked` scan.
    tracked_gen: Option<u64>,
    /// `Notify` calls handled last poll and their ids; they reply on the next one, after the
    /// `notification` handlers have settled. One dropped unanswered fails.
    replies: Vec<(async_channel::Sender<u32>, u32)>,
    /// The next command, held while its image hint decodes.
    held: Option<Cmd>,
}

/// The owner watch, handed over by the start job; `ended` once the instance is gone, so a watch
/// made after that is ended at once.
#[derive(Default)]
struct Watch {
    cancel: Option<crate::dbus::Cancel>,
    ended: bool,
    /// Set once the instance is gone: ends a wait for the session bus to appear.
    stop: Arc<crate::svc::Stop>,
}

/// The instance goes with the last `NotificationServer` (or its World): its object leaves the
/// bus and the name is released, so another daemon can take it, unless a later instance has
/// started since and serves them now.
impl Drop for Notifs {
    fn drop(&mut self) {
        let (watch, stop) = {
            let mut w = self.watch.lock().unwrap();
            w.ended = true;
            (w.cancel.take(), w.stop.clone())
        };
        stop.store(true, Ordering::Release);
        if let Some(w) = watch {
            w.cancel();
        }
        let instance = self.instance;
        BUS.run(move || {
            if ACTIVE.load(Ordering::Acquire) != instance {
                return;
            }
            ACTIVE.store(0, Ordering::Release);
            let Some(c) = crate::dbus::session() else {
                return;
            };
            let _ = c.object_server().remove::<Server, _>(PATH);
            match crate::sysbus::within(c.inner().release_name(NAME)) {
                Ok(true) => elog::eprintln!("notifications: unregistered notification server"),
                Ok(false) => {}
                Err(e) => elog::eprintln!("notifications: cannot release {NAME}: {e}"),
            }
        });
    }
}

impl Notifs {
    /// A new id for `replaces_id` 0: the counter's next one not held by a live notification
    /// (a `replaces_id` may have shown one under an id the counter has not reached yet).
    fn fresh_id(&mut self) -> u32 {
        let mut id = self.next_id;
        while self.live.contains_key(&id) {
            id = id.wrapping_add(1).max(1);
        }
        self.next_id = id.wrapping_add(1).max(1);
        id
    }

    /// Whether the server has requested its name and watches for the name's owner to leave.
    pub fn watching(&self) -> bool {
        self.watch.lock().unwrap().cancel.is_some()
    }

    /// Answers the `Notify` calls handled last poll.
    fn reply(&mut self) {
        for (done, id) in self.replies.drain(..) {
            let _ = done.try_send(id);
        }
    }

    fn emit<B>(&self, member: &str, body: &B)
    where
        B: serde::Serialize + zbus::zvariant::DynamicType,
    {
        if let Some(c) = &self.conn {
            let _ = c.emit_signal(None::<&str>, PATH, IFACE, member, body);
        }
    }
}

/// Logs the RequestName reply `r`, like quickshell's `tryRegister`.
/// `RequestName`, ended with an error when the bus daemon does not answer
/// ([`crate::sysbus::REPLY_TIMEOUT`]), so what is queued on [`BUS`] behind it runs.
fn request_name(c: &zbus::blocking::Connection) -> zbus::Result<zbus::fdo::RequestNameReply> {
    let flags = zbus::fdo::RequestNameFlags::DoNotQueue.into();
    crate::sysbus::within(c.inner().request_name_with_flags(NAME, flags))
}

fn registered(r: zbus::Result<zbus::fdo::RequestNameReply>) {
    match r {
        Ok(
            zbus::fdo::RequestNameReply::PrimaryOwner | zbus::fdo::RequestNameReply::AlreadyOwner,
        ) => elog::eprintln!("notifications: registered notification server with dbus"),
        r => elog::eprintln!(
            "notifications: could not register notification server at {NAME}, presumably because one is already registered ({:?}); registration will be attempted again if the active service is unregistered",
            r.map(|_| ()).err()
        ),
    }
}

fn start(h: &crate::svc::Handle) -> Notifs {
    let instance = NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed);
    let support = Arc::new(Mutex::new(Support::default()));
    let watch = Arc::new(Mutex::new(Watch::default()));
    let (tx, rx) = h.channel();
    let serving = Serving {
        instance,
        support: support.clone(),
        tx,
        watch: watch.clone(),
    };
    BUS.run(move || {
        ACTIVE.store(instance, Ordering::Release);
        serve(serving);
    });
    Notifs {
        instance,
        conn: None,
        watch,
        rx,
        next_id: 1,
        support,
        live: BTreeMap::new(),
        order: vec![],
        tracked_gen: None,
        replies: vec![],
        held: None,
    }
}

/// What serving an instance on a connection takes, kept for every connection it serves on.
#[derive(Clone)]
struct Serving {
    instance: u64,
    support: Arc<Mutex<Support>>,
    tx: crate::svc::Sender<Cmd>,
    watch: Arc<Mutex<Watch>>,
}

impl Serving {
    /// Whether this instance still serves: not gone, and no later instance has started.
    fn current(&self) -> bool {
        !self.watch.lock().unwrap().ended && ACTIVE.load(Ordering::Acquire) == self.instance
    }
}

/// Held by the owner watch's sink, which the bus drops when the watch ends: with the connection
/// (the shared one reconnects on its next use) the instance serves again on the next one, as the
/// tray does. A watch its instance ended starts nothing.
struct Reconnect(Option<Serving>);

impl Drop for Reconnect {
    fn drop(&mut self) {
        if let Some(s) = self.0.take() {
            BUS.run(move || serve(s));
        }
    }
}

/// Serves the object on the shared session connection, then watches the name and requests it,
/// like quickshell's NotificationServer on `QDBusConnection::sessionBus()`. Connecting,
/// serving, AddMatch and RequestName all wait on the bus daemon: run on [`BUS`], the main loop
/// only gets the connection once it serves. Without a bus it serves once the bus socket appears.
fn serve(s: Serving) {
    if !s.current() {
        return;
    }
    // Set up before the connect, so a socket created in between is not missed.
    let socket = crate::sysbus::session_socket()
        .as_deref()
        .and_then(crate::sysbus::SocketWatch::new);
    let server = Server {
        support: s.support.clone(),
        tx: Mutex::new(s.tx.clone()),
    };
    let conn = crate::dbus::session()
        .ok_or_else(|| zbus::Error::Failure("no session bus".into()))
        .and_then(|c| {
            // A previous instance's server (an earlier World in this process) is replaced.
            let _ = c.object_server().remove::<Server, _>(PATH);
            let r = c.object_server().at(PATH, server);
            r.map(|_| c)
        });
    let conn = match conn {
        Ok(c) => c,
        Err(e) => {
            elog::eprintln!(
                "notifications: could not connect to dbus, notification service will not work until it is available: {e}"
            );
            if let Some(w) = socket {
                serve_when_the_bus_appears(w, s);
            }
            return;
        }
    };
    drop(socket);
    if s.tx.send(Cmd::Ready(conn.clone())).is_err() {
        return;
    }
    let instance = s.instance;
    // QDBusServiceWatcher(WatchForUnregistration) → tryRegister again. Watched before the
    // first request, so an owner leaving in between is not missed.
    let sink: crate::dbus::Sink = {
        let conn = conn.clone();
        let reconnect = Reconnect(Some(s.clone()));
        Box::new(move |m| {
            let _ = &reconnect;
            if let Ok((_, _, new)) = m.body().deserialize::<(String, String, String)>()
                && new.is_empty()
            {
                // Not on the router thread: the reply comes through it.
                let c = conn.clone();
                BUS.run(move || {
                    if ACTIVE.load(Ordering::Acquire) == instance {
                        elog::eprintln!(
                            "notifications: active notification server unregistered, attempting registration"
                        );
                        registered(request_name(&c));
                    }
                });
            }
            true
        })
    };
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender("org.freedesktop.DBus")
        .and_then(|b| b.interface("org.freedesktop.DBus"))
        .and_then(|b| b.member("NameOwnerChanged"))
        .and_then(|b| b.add_arg(NAME))
        .map(|b| b.build());
    let watch = match rule
        .map(|r| crate::dbus::subscribe_to_cancellable(crate::dbus::Bus::Session, vec![r], sink))
    {
        Ok(Some((c, w))) if c.unique_name() == conn.unique_name() => Some(w),
        // The served connection closed before the watch: the sink, dropped with its
        // subscription on the next connection, serves there.
        Ok(Some((_, w))) => {
            w.cancel();
            return;
        }
        Ok(None) => {
            elog::eprintln!("notifications: cannot watch {NAME} owner: no session bus");
            None
        }
        Err(e) => {
            elog::eprintln!("notifications: cannot watch {NAME} owner: {e}");
            None
        }
    };
    registered(request_name(&conn));
    // Handed to the instance, which ends the watch when it goes; ended here when the instance is
    // gone already.
    if let Some(w) = watch {
        let mut st = s.watch.lock().unwrap();
        if st.ended {
            drop(st);
            w.cancel();
        } else {
            st.cancel = Some(w);
            drop(st);
            let _ = s.tx.send(Cmd::Watching);
        }
    }
}

/// Serves `s` on the first event of the session bus socket `w` watches (created, moved in or
/// touched after listen(2)), or never once the instance is gone ([`Watch::stop`]). Waits for
/// that event, never a timer.
fn serve_when_the_bus_appears(mut w: crate::sysbus::SocketWatch, s: Serving) {
    let stop = s.watch.lock().unwrap().stop.clone();
    w.stop_on(&stop);
    let spawned = std::thread::Builder::new()
        .name("ri-notif-bus".into())
        .spawn(move || {
            if w.wait() {
                BUS.run(move || serve(s));
            }
        });
    if let Err(e) = spawned {
        elog::eprintln!("notifications: cannot wait for the session bus: {e}");
    }
}

/// `QVariant::toBool` of a hint.
fn v_bool(v: Option<&V>) -> bool {
    match v {
        Some(V::B(b)) => *b,
        Some(V::I(i)) => *i != 0,
        Some(V::F(f)) => *f != 0.0,
        Some(V::S(s)) => !(s.is_empty() || s == "0" || s.eq_ignore_ascii_case("false")),
        _ => false,
    }
}

/// `QVariant::toString` of a hint.
fn v_str(v: Option<&V>) -> String {
    match v {
        Some(V::S(s)) => s.clone(),
        Some(V::B(b)) => b.to_string(),
        Some(V::I(i)) => i.to_string(),
        Some(V::F(f)) => f.to_string(),
        _ => String::new(),
    }
}

fn to_value(v: &V) -> Value {
    match v {
        V::S(s) => Value::str(s),
        V::I(i) => Value::Num(*i as f64),
        V::F(f) => Value::Num(*f),
        V::B(b) => Value::Bool(*b),
        V::L(l) => Value::array(l.iter().map(to_value).collect()),
        V::M(m) => {
            let o = crate::eval::new_obj();
            for (k, v) in m {
                crate::eval::set_own(&o, k, to_value(v));
            }
            Value::Obj(o)
        }
        V::Nil => Value::Undefined,
    }
}

fn hint<'a>(h: &'a [(String, V)], k: &str) -> Option<&'a V> {
    h.iter().find(|e| e.0 == k).map(|e| &e.1)
}

fn read_bool(w: &mut World, i: usize, k: &str) -> bool {
    w.read(i, k).is_some_and(|v| v.truthy())
}

/// quickshell `Notification::updateProperties`.
fn update(w: &mut World, live: &mut Live, r: Req, inline_reply: bool, first: bool) {
    let item = live.item;
    let h = &r.hints;
    let desktop = v_str(hint(h, "desktop-entry"));
    let mut app_icon = r.app_icon;
    if app_icon.is_empty()
        && !desktop.is_empty()
        && let Value::Item(e) = crate::desktop::by_id(w, &desktop)
    {
        app_icon = w
            .read(e, "icon")
            .map(|v| v.to_js_string())
            .unwrap_or_default();
    }
    let mut image = String::new();
    let mut has_data = false;
    {
        let mut g = IMAGES.lock().unwrap();
        let map = g.get_or_insert_with(HashMap::new);
        // The previous image's copy is stale: a newer image or none replaces it in the history.
        if let Some(old) = live.saved.take() {
            forget_saved(&old);
        }
        match r.image.and_then(|p| p.img) {
            Some(img) => {
                // NotificationImage::writeImage bumps the change index on every update.
                live.change += 1;
                image = format!("image://qsimage/{}-{}/{}", run(), live.handle, live.change);
                has_data = !img.px.is_empty();
                let img = Arc::new(img);
                if has_data && let Some(path) = saved_path(&image) {
                    // PNG encoding is disk work: off the main loop.
                    disk(DiskOp::Save(path, img.clone()));
                    live.saved = Some(image.clone());
                }
                map.insert(live.handle, img);
            }
            None => {
                map.remove(&live.handle);
            }
        }
    }
    if !has_data && let Some(p) = hint(h, "image-path").or_else(|| hint(h, "image_path")) {
        let p = match p {
            V::S(s) => s.clone(),
            _ => String::new(),
        };
        image = if p.starts_with("file:") {
            p
        } else {
            format!("image://icon/{p}")
        };
    }
    let urgency = match hint(h, "urgency") {
        Some(V::I(u)) => *u as u8 as f64,
        Some(V::B(b)) => *b as u8 as f64,
        Some(_) => 0.0,
        None => 1.0,
    };
    for (k, v) in [
        ("expireTimeout", Value::Num(r.expire as f64)),
        ("appName", Value::str(&r.app_name)),
        ("summary", Value::str(&r.summary)),
        ("body", Value::str(&r.body)),
        (
            "hasActionIcons",
            Value::Bool(v_bool(hint(h, "action-icons"))),
        ),
        ("resident", Value::Bool(v_bool(hint(h, "resident")))),
        ("transient", Value::Bool(v_bool(hint(h, "transient")))),
        ("desktopEntry", Value::str(&desktop)),
        ("urgency", Value::Num(urgency)),
        ("appIcon", Value::str(&app_icon)),
        ("image", Value::str(&image)),
    ] {
        w.write(item, k, v);
    }
    // QVariantMap compares by value: only a different map notifies hintsChanged. Both lists
    // are sorted by key (`Notify`).
    if first || live.hints != r.hints {
        w.write(item, "hints", to_value(&V::M(r.hints.clone())));
        live.hints = r.hints;
    }
    if !r.actions.len().is_multiple_of(2) {
        elog::eprintln!(
            "notifications: notification {} ({}) sent an action set of an invalid length",
            w.read(item, "id").map_or(0.0, |v| v.to_number()),
            r.app_name
        );
        return;
    }
    let mut changed = false;
    let mut deleted = vec![];
    let mut ai = 0;
    for pair in r.actions.chunks(2) {
        let (ident, text) = (&pair[0], &pair[1]);
        if ident == "inline-reply" && inline_reply {
            if read_bool(w, item, "hasInlineReply") {
                elog::eprintln!(
                    "notifications: {} sent an action set with duplicate inline-reply actions",
                    r.app_name
                );
            } else {
                w.write(item, "hasInlineReply", Value::Bool(true));
                w.write(item, "inlineReplyPlaceholder", Value::str(text));
            }
            continue;
        }
        let cur = live.actions.get(ai).copied();
        match cur {
            Some(a)
                if w.read(a, "identifier")
                    .is_some_and(|v| v.to_js_string() == *ident) =>
            {
                w.write(a, "text", Value::str(text));
            }
            _ => {
                let a = crate::types::native_singleton(w, "NotificationAction");
                w.write(a, "identifier", Value::str(ident));
                w.write(a, "text", Value::str(text));
                match cur {
                    Some(old) => {
                        deleted.push(old);
                        live.actions[ai] = a;
                    }
                    None => live.actions.push(a),
                }
                changed = true;
            }
        }
        ai += 1;
    }
    while live.actions.len() > ai {
        deleted.push(live.actions.pop().unwrap());
        changed = true;
    }
    if changed {
        let v = live.actions.iter().map(|a| Value::Item(*a)).collect();
        w.write(item, "actions", Value::array(v));
    }
    for a in deleted {
        w.destroy(a);
    }
}

/// `NotificationServerQml` support flags from the first live `NotificationServer`.
fn read_support(w: &mut World, s: usize) -> Support {
    let b = |w: &mut World, k: &str| read_bool(w, s, k);
    let extra = match w.read(s, "extraHints") {
        Some(Value::Obj(o)) => o
            .borrow()
            .array
            .as_ref()
            .map(|a| a.iter().map(|v| v.to_js_string()).collect())
            .unwrap_or_default(),
        _ => vec![],
    };
    Support {
        persistence: b(w, "persistenceSupported"),
        body: b(w, "bodySupported"),
        body_markup: b(w, "bodyMarkupSupported"),
        body_hyperlinks: b(w, "bodyHyperlinksSupported"),
        body_images: b(w, "bodyImagesSupported"),
        actions: b(w, "actionsSupported"),
        action_icons: b(w, "actionIconsSupported"),
        image: b(w, "imageSupported"),
        inline_reply: b(w, "inlineReplySupported"),
        extra_hints: extra,
    }
}

/// QML-side methods. `dismiss()`/`expire()` mark the object untracked at once (quickshell's
/// `close()` sets mCloseReason synchronously); the removal and signals happen in the poller.
pub(crate) fn method(it: &mut crate::eval::Interp, this: &Value, m: &str, a: &[Value]) {
    let Some(w) = crate::runtime::world_of(it) else {
        return;
    };
    let mut w = w.borrow_mut();
    let Value::Item(i) = this else { return };
    if w.dead.contains(i) {
        elog::eprintln!("notifications: cannot {m} destroyed notification");
        return;
    }
    let reason = match m {
        "expire" => EXPIRED,
        "dismiss" => DISMISSED,
        _ => {
            let mut v = vec![this.clone()];
            v.extend_from_slice(a);
            w.effects
                .push(crate::svc::Effect::Notif(crate::svc::Call::new(m, v)));
            return;
        }
    };
    set_reason(&mut w, *i, reason);
    w.effects
        .push(crate::svc::Effect::Notif(crate::svc::Call::new(
            "close",
            vec![this.clone(), Value::Num(reason as f64)],
        )));
}

fn set_reason(w: &mut World, i: usize, reason: u32) {
    if !w.items[i].props.contains_key("\x01closeReason") {
        w.declare(i, "\x01closeReason", "real");
    }
    w.write(i, "\x01closeReason", Value::Num(reason as f64));
    w.write(i, "tracked", Value::Bool(reason == 0));
}

fn close_reason(w: &mut World, i: usize) -> u32 {
    match w.read(i, "\x01closeReason") {
        Some(Value::Num(n)) => n as u32,
        // A fresh Notification starts with mCloseReason = Dismissed.
        _ if read_bool(w, i, "tracked") => 0,
        _ => DISMISSED,
    }
}

impl Runtime {
    pub(crate) fn poll_notifications(&mut self, calls: Vec<crate::svc::Call>) -> bool {
        let servers = {
            let mut w = self.world.borrow_mut();
            let s = w.of_type("NotificationServer");
            for &i in &s {
                w.mirror(i, crate::svc::Svc::Notif);
            }
            if s.is_empty() {
                // No server item left: the name goes with the instance, for another daemon to
                // take, and is requested again once a server is made.
                let Some(n) = w.notifications.take() else {
                    return false;
                };
                w.notif_backlog = false;
                drop(w);
                self.stop_notifs(n);
                return true;
            }
            if w.notifications.is_none() {
                // NotificationServerSupport::body defaults to true (qmltypes default is false).
                for &i in &s {
                    if !w.is_assigned(i, "bodySupported") {
                        w.write(i, "bodySupported", Value::Bool(true));
                    }
                }
                w.notifications = Some(start(&w.waker.handle(crate::svc::Svc::Notif)));
            }
            s
        };
        let mut n = self.world.borrow_mut().notifications.take().unwrap();
        let changed = self.poll_notifs(&mut n, &servers, calls);
        self.world.borrow_mut().notifications = Some(n);
        changed
    }

    fn poll_notifs(
        &mut self,
        n: &mut Notifs,
        servers: &[usize],
        calls: Vec<crate::svc::Call>,
    ) -> bool {
        let mut changed = false;
        n.reply();
        if let Some(&s) = servers.first() {
            let sup = read_support(&mut self.world.borrow_mut(), s);
            let mut cur = n.support.lock().unwrap();
            if *cur != sup {
                *cur = sup;
            }
        }
        changed |= self.notif_calls(n, servers, calls, None);
        // Like Qt delivering one D-Bus message per event, a flood must not starve the rest of
        // the loop (IPC, input, frames): one command per loop turn, in order; a further one
        // wakes the loop again.
        let mut next = n.held.take().or_else(|| n.rx.try_recv().ok());
        if decoding(&mut next) {
            n.held = next.take();
        }
        if let Some(c) = next {
            changed = true;
            match c {
                Cmd::Ready(c) => n.conn = Some(c),
                Cmd::Watching => {}
                Cmd::Close(id, done) => {
                    let live = n.live.contains_key(&id);
                    self.close_many(n, servers, vec![(id, CLOSE_REQUESTED)]);
                    let _ = done.try_send(live);
                }
                Cmd::Notify(mut r) => {
                    let done = r.done.take();
                    let id = self.notify(n, servers, *r);
                    n.replies.extend(done.map(|tx| (tx, id)));
                }
            }
            n.held = n.rx.try_recv().ok();
        }
        // A request whose image still decodes is woken for by its decoder.
        let backlog = n.held.is_some() && !decoding(&mut n.held);
        if backlog {
            self.world.borrow().waker.wake(crate::svc::Svc::Notif);
        }
        self.world.borrow_mut().notif_backlog = backlog;
        if !n.replies.is_empty() {
            self.world.borrow().waker.wake(crate::svc::Svc::Notif);
        }
        // `notification.tracked = false` is equivalent to dismiss().
        let untracked: Vec<(u32, u32)> = {
            let mut w = self.world.borrow_mut();
            if n.tracked_gen == Some(w.tracked_gen) {
                vec![]
            } else {
                let v = n
                    .order
                    .iter()
                    .filter(|id| !read_bool(&mut w, n.live[*id].item, "tracked"))
                    .map(|id| (*id, DISMISSED))
                    .collect();
                n.tracked_gen = Some(w.tracked_gen);
                v
            }
        };
        if !untracked.is_empty() {
            self.close_many(n, servers, untracked);
            changed = true;
        }
        changed
    }

    /// Handle queued `notif.*` effects; `fresh` is a notification still inside its
    /// `notification` signal (only its close reason is recorded, like quickshell's close()).
    fn notif_calls(
        &mut self,
        n: &mut Notifs,
        servers: &[usize],
        calls: Vec<crate::svc::Call>,
        fresh: Option<usize>,
    ) -> bool {
        let mut closes = vec![];
        let changed = !calls.is_empty();
        for crate::svc::Call { method: k, args: a } in calls {
            let Some(Value::Item(i)) = a.first() else {
                continue;
            };
            let i = *i;
            match k.as_str() {
                "close" => {
                    if let Some(id) = n.live.iter().find(|e| e.1.item == i).map(|e| *e.0) {
                        closes.push((
                            id,
                            a.get(1).map_or(DISMISSED as f64, |v| v.to_number()) as u32,
                        ));
                    }
                }
                "invoke" => {
                    let Some((id, item)) = n
                        .live
                        .iter()
                        .find(|e| e.1.actions.contains(&i))
                        .map(|e| (*e.0, e.1.item))
                    else {
                        if Some(i) != fresh {
                            elog::eprintln!("notifications: cannot invoke destroyed notification");
                        }
                        continue;
                    };
                    let ident = self
                        .world
                        .borrow_mut()
                        .read(i, "identifier")
                        .map(|v| v.to_js_string())
                        .unwrap_or_default();
                    n.emit("ActionInvoked", &(id, ident.as_str()));
                    let mut w = self.world.borrow_mut();
                    if !read_bool(&mut w, item, "resident") {
                        set_reason(&mut w, item, DISMISSED);
                        closes.push((id, DISMISSED));
                    }
                }
                "sendInlineReply" => {
                    let text = a.get(1).map(|v| v.to_js_string()).unwrap_or_default();
                    if !n.support.lock().unwrap().inline_reply {
                        elog::eprintln!("notifications: inline reply support disabled on server");
                        continue;
                    }
                    let mut w = self.world.borrow_mut();
                    if !read_bool(&mut w, i, "hasInlineReply") {
                        elog::eprintln!(
                            "notifications: cannot send reply to notification without inline-reply action"
                        );
                        continue;
                    }
                    let Some(id) = n.live.iter().find(|e| e.1.item == i).map(|e| *e.0) else {
                        elog::eprintln!(
                            "notifications: cannot send reply to destroyed notification"
                        );
                        continue;
                    };
                    n.emit("NotificationReplied", &(id, text.as_str()));
                    if !read_bool(&mut w, i, "resident") {
                        set_reason(&mut w, i, DISMISSED);
                        closes.push((id, DISMISSED));
                    }
                }
                _ => {}
            }
        }
        if !closes.is_empty() {
            self.close_many(n, servers, closes);
        }
        changed
    }

    fn take_calls(&mut self) -> Vec<crate::svc::Call> {
        let mut w = self.world.borrow_mut();
        let mut mine = vec![];
        w.effects.retain_mut(|e| match e {
            crate::svc::Effect::Notif(c) => {
                mine.push(std::mem::replace(c, crate::svc::Call::new("", vec![])));
                false
            }
            _ => true,
        });
        mine
    }

    /// The last server item is gone: the calls already handled reply, the tracked notifications
    /// close (nothing is left to show them) and the instance is dropped, which releases the name.
    /// Requests still queued fail.
    fn stop_notifs(&mut self, mut n: Notifs) {
        n.reply();
        for id in std::mem::take(&mut n.order) {
            if let Some(live) = n.live.remove(&id) {
                n.emit("NotificationClosed", &(id, SERVER_GONE));
                self.drop_live(live);
            }
        }
    }

    /// `Notify`: a live `replaces_id` is updated in place. Any other non-zero `replaces_id`
    /// (closed, even by a close handled just before this request, or never shown) becomes a new
    /// notification under that same id, since the spec's reply to a non-zero `replaces_id` is
    /// that id; a 0 gets the next free id. Returns the id `Notify` replies with.
    fn notify(&mut self, n: &mut Notifs, servers: &[usize], r: Req) -> u32 {
        let inline = n.support.lock().unwrap().inline_reply;
        if r.replaces != 0
            && let Some(mut live) = n.live.remove(&r.replaces)
        {
            let id = r.replaces;
            update(&mut self.world.borrow_mut(), &mut live, r, inline, false);
            n.live.insert(id, live);
            crate::notifier::replaced(&mut self.world.borrow_mut(), id);
            self.settle();
            let calls = self.take_calls();
            self.notif_calls(n, servers, calls, None);
            return id;
        }
        let id = match r.replaces {
            0 => n.fresh_id(),
            id => id,
        };
        let item = {
            let mut w = self.world.borrow_mut();
            let item = crate::types::native_singleton(&mut w, "Notification");
            w.write(item, "id", Value::Num(id as f64));
            w.write(item, "lastGeneration", Value::Bool(false));
            item
        };
        let mut live = Live {
            item,
            actions: vec![],
            handle: NEXT_HANDLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1,
            change: 0,
            hints: vec![],
            saved: None,
        };
        update(&mut self.world.borrow_mut(), &mut live, r, inline, true);
        {
            let mut w = self.world.borrow_mut();
            for &s in servers {
                crate::native::emit(
                    &mut w,
                    s,
                    "notification",
                    &["notification"],
                    vec![Value::Item(item)],
                );
            }
        }
        self.settle();
        let calls = self.take_calls();
        self.notif_calls(n, servers, calls, Some(item));
        let tracked = read_bool(&mut self.world.borrow_mut(), item, "tracked");
        if tracked {
            n.live.insert(id, live);
            n.order.push(id);
            self.publish(n, servers);
        } else {
            let reason = close_reason(&mut self.world.borrow_mut(), item);
            n.emit("NotificationClosed", &(id, reason));
            self.drop_live(live);
        }
        id
    }

    /// A closed notification leaves the original's history too (its wrapper's `notification` turns null
    /// and the original discards it), so its saved copy goes with the decoded pixels.
    fn drop_live(&mut self, live: Live) {
        if let Some(m) = IMAGES.lock().unwrap().as_mut() {
            m.remove(&live.handle);
        }
        if let Some(src) = &live.saved {
            forget_saved(src);
        }
        let mut w = self.world.borrow_mut();
        // A slot the destroy did not release yet drops its property storage (hints, strings)
        // now, so a notification flood does not retain memory. Reads of dead items yield null.
        for i in live.actions.into_iter().chain([live.item]) {
            w.destroy(i);
            if let Some(it) = w.items.get_mut(i) {
                it.props = Default::default();
            }
        }
    }

    fn publish(&mut self, n: &Notifs, servers: &[usize]) {
        let vals: Vec<Value> = n
            .order
            .iter()
            .map(|id| Value::Item(n.live[id].item))
            .collect();
        let mut w = self.world.borrow_mut();
        for &s in servers {
            if let Some(Value::Item(m)) = w.read(s, "trackedNotifications") {
                w.write(m, "values", Value::array(vals.clone()));
            }
        }
    }

    /// quickshell `NotificationServer::deleteNotification`: `closed(reason)` runs first, then
    /// the object leaves trackedNotifications, NotificationClosed goes out and it is destroyed.
    fn close_many(&mut self, n: &mut Notifs, servers: &[usize], mut list: Vec<(u32, u32)>) {
        loop {
            let mut seen = HashSet::new();
            list.retain(|(id, _)| n.live.contains_key(id) && seen.insert(*id));
            if list.is_empty() {
                return;
            }
            {
                let mut w = self.world.borrow_mut();
                for &(id, reason) in &list {
                    let item = n.live[&id].item;
                    if reason != 0 {
                        w.write(item, "tracked", Value::Bool(false));
                    }
                    crate::native::emit(
                        &mut w,
                        item,
                        "closed",
                        &["reason"],
                        vec![Value::Num(reason as f64)],
                    );
                }
            }
            self.settle();
            // One model update for the whole batch ("clear all" is one rewrite, not one per id),
            // before the closed objects are destroyed.
            let mut gone = Vec::with_capacity(list.len());
            for &(id, reason) in &list {
                if let Some(live) = n.live.remove(&id) {
                    gone.push((id, reason, live));
                }
            }
            n.order.retain(|x| n.live.contains_key(x));
            self.publish(n, servers);
            for (id, reason, live) in gone {
                n.emit("NotificationClosed", &(id, reason));
                self.drop_live(live);
            }
            // Handlers of `closed` may have closed others.
            let calls = self.take_calls();
            list.clear();
            for crate::svc::Call { method: k, args: a } in calls {
                if k == "close"
                    && let Some(Value::Item(i)) = a.first()
                    && let Some(id) = n.live.iter().find(|e| e.1.item == *i).map(|e| *e.0)
                {
                    list.push((
                        id,
                        a.get(1).map_or(DISMISSED as f64, |v| v.to_number()) as u32,
                    ));
                } else {
                    self.world
                        .borrow_mut()
                        .effects
                        .push(crate::svc::Effect::Notif(crate::svc::Call::new(k, a)));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    /// The saved-copy bound is kept by the in-memory list: past [`super::SAVED_MAX`] the oldest
    /// copy goes, a re-saved copy counts as newest, a removed one frees its slot.
    #[test]
    fn saved_copies_bounded_oldest_first() {
        let p = |i: usize| std::path::PathBuf::from(format!("{i}.png"));
        let mut s = super::Saved::default();
        for i in 0..super::SAVED_MAX {
            assert!(s.add(p(i)).is_empty());
        }
        assert!(s.add(p(0)).is_empty(), "re-saving does not grow the list");
        assert_eq!(s.add(p(1000)), vec![p(1)]);
        s.remove(&p(2));
        assert!(s.add(p(1001)).is_empty());
        assert_eq!(s.add(p(1002)), vec![p(3)]);
        assert_eq!(s.0.len(), super::SAVED_MAX);
    }

    use super::*;

    fn hint(w: i32, h: i32, stride: i32, alpha: bool, data: Vec<u8>) -> crate::dbusvar::Var {
        crate::dbusvar::Var::Image(crate::dbusvar::ImageData {
            w,
            h,
            stride,
            alpha,
            bits: 8,
            chans: if alpha { 4 } else { 3 },
            data,
        })
    }

    /// The sender's w/h/rowstride are checked against the bytes it sent: a tiny buffer claiming
    /// a huge image is dropped instead of being allocated and walked (w*h pixels).
    /// docs/quickshell-differences.md: an image of 1024 a side is kept as it came, one of 1025
    /// is averaged down to 1024.
    #[test]
    fn an_image_is_kept_up_to_1024_a_side() {
        // Opaque, blue all or nothing in turn: averaged, it is neither.
        let sent = |n: usize| -> Vec<u8> {
            let px = (0..n * n).flat_map(|i| [0, 0, ((i % n + i / n) % 2 * 255) as u8, 255]);
            px.collect()
        };
        let kept = image_hint(&hint(1024, 1024, 4096, true, sent(1024)));
        assert_eq!((kept.w, kept.h), (1024, 1024));
        let same = kept
            .px
            .iter()
            .zip(sent(1024).chunks(4))
            .all(|(k, s)| k == s);
        assert!(same, "1024x1024 is changed");
        let averaged = image_hint(&hint(1025, 1025, 4100, true, sent(1025)));
        assert_eq!((averaged.w, averaged.h), (1024, 1024));
        let blue = averaged.px.iter().any(|p| ![0, 255].contains(&p[2]));
        assert!(blue, "1025x1025 is not averaged");
    }

    #[test]
    fn image_hint_sizes_bounded_by_data() {
        let huge = image_hint(&hint(1 << 30, 1 << 30, 0, true, vec![1, 2, 3, 4]));
        assert!(huge.px.is_empty() && huge.w == 0, "{}x{}", huge.w, huge.h);
        // One row short (old code padded the missing pixels with zeros).
        let short = image_hint(&hint(2, 2, 8, true, vec![9; 12]));
        assert_eq!((short.w, short.h, short.px.len()), (0, 0, 0));
        // A rowstride shorter than a row would overlap rows.
        let overlap = image_hint(&hint(2, 2, 4, true, vec![9; 16]));
        assert_eq!(overlap.px.len(), 0);
        // The last row may omit its padding; a valid image still decodes.
        let ok = image_hint(&hint(1, 2, 4, false, vec![1, 2, 3, 0, 4, 5, 6]));
        assert_eq!((ok.w, ok.h), (1, 2));
        assert_eq!(ok.px, vec![[1, 2, 3, 255], [4, 5, 6, 255]]);
    }
}
