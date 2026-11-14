//! Native `Rustshell.Services.Pipewire`: nodes, defaults and volumes (node Props or the
//! backing device's Route, cube-root "visual" volumes like quickshell).

use crate::eval::{new_obj, set_own};
use crate::runtime::{Runtime, World};
use crate::value::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NodeSnap {
    pub id: u32,
    /// What tells two nodes apart ([`node_key`]): PipeWire hands a removed object's id to the
    /// next one, and a restarted daemon starts its ids and `object.serial`s over, so neither
    /// (id, serial) nor a serial alone names one node for the life of the client.
    pub key: u64,
    /// The registry global's `object.serial`, when it carries one.
    pub serial: Option<u64>,
    pub name: String,
    pub desc: String,
    pub nick: String,
    pub ty: u8,
    pub props: Vec<(String, String)>,
    pub volumes: Vec<f32>,
    pub channels: Vec<u32>,
    pub muted: bool,
    /// quickshell `PwNode::ready`: the core sync issued after the node's first info (and the
    /// Props enumeration it starts) is done, so its initial volumes have been delivered.
    pub ready: bool,
    /// The last volume and mute writes ([`Cmd`] `write`) the server has reported the outcome
    /// of: `volumes` and `muted` show their result, or a later change.
    pub volumes_ack: u64,
    pub muted_ack: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Snap {
    /// Shared with the backend: a node's entry is rebuilt only when that node changes.
    pub nodes: Vec<Arc<NodeSnap>>,
    /// default.audio.sink / source, default.configured.audio.sink / source (node names).
    pub defaults: [String; 4],
    /// The state of a connected daemon, published only once its initial objects (globals,
    /// their info, params and the `default` metadata) have all arrived; `false` for the empty
    /// snapshot of a daemon that could not be reached or went away.
    pub connected: bool,
    /// [`NodeSnap::volumes_ack`] for the `default.configured.audio.sink` / `source` writes.
    pub configured_ack: [u64; 2],
}

/// A key no other node of this process has had: the backend gives one to every node global
/// it binds, across reconnects.
pub fn node_key() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// The metadata keys of [`Cmd::Default`], in `Snap::configured_ack` order.
pub const CONFIGURED: [&str; 2] = [
    "default.configured.audio.sink",
    "default.configured.audio.source",
];

/// QML writes for the backend. Each carries a `write` id, increasing across all writes, that
/// the backend acks in the snapshot once the server reported its outcome: until then a
/// snapshot still shows the value from before it (or an earlier write's echo), which must not
/// replace the newer value QML shows. Node commands name the node by id and [`NodeSnap::key`]: a
/// write on a node that has since been removed (or belonged to a previous daemon) must not
/// reach another node that got its id.
pub enum Cmd {
    Volumes {
        id: u32,
        key: u64,
        write: u64,
        volumes: Vec<f32>,
    },
    Muted {
        id: u32,
        key: u64,
        write: u64,
        muted: bool,
    },
    /// Sets [`CONFIGURED`]`[slot]` to the node `name`, or clears the key (`None`).
    Default {
        slot: usize,
        write: u64,
        name: Option<String>,
    },
}

impl Cmd {
    pub fn write(&self) -> u64 {
        match self {
            Cmd::Volumes { write, .. } | Cmd::Muted { write, .. } | Cmd::Default { write, .. } => {
                *write
            }
        }
    }
}

type Shared = Arc<(Mutex<Option<Snap>>, AtomicBool, crate::svc::Handle)>;

struct Node {
    item: usize,
    audio: Option<usize>,
    snap: Arc<NodeSnap>,
    vol: f64,
    muted: bool,
    volumes: Vec<f32>,
    /// The ids of the last volume and mute writes sent for this node.
    volumes_write: u64,
    muted_write: u64,
    /// Arrival order: quickshell appends each new node to `Pipewire.nodes` (ObjectModel).
    seq: u64,
}

pub struct Pw {
    shared: Shared,
    send: Box<dyn Fn(Cmd)>,
    nodes: HashMap<u32, Node>,
    conf: [Option<usize>; 2],
    /// The ids of the last `conf` writes sent.
    conf_write: [u64; 2],
    /// The last write id handed out.
    writes: u64,
    /// Nodes the backend reported ready whose `ready` is written on the next poll, after the
    /// handlers of the snapshot's volume writes ran (quickshell emits them in earlier dispatches).
    unready: Vec<usize>,
    seq: u64,
}

fn toggle_mute(it: &mut crate::eval::Interp, _: &Value, _: &[Value]) -> crate::eval::R<Value> {
    if let Some(w) = crate::runtime::world_of(it) {
        let mut w = w.borrow_mut();
        if let Some(&root) = w.singletons.get("Pipewire")
            && let Some(mut pw) = w.pw.take()
        {
            pw.toggle_default_source_mute(&mut w, root);
            w.pw = Some(pw);
        }
    }
    Ok(Value::Undefined)
}

/// The `Microphone` singleton of module `Impulse` (`Impulse/microphone.qmltypes`).
pub static MICROPHONE: crate::impulse::Type = crate::impulse::Type {
    name: "Microphone",
    methods: &[("toggleMute", toggle_mute)],
    init: None,
};

/// quickshell `PwNodeType` flags from `media.class`.
pub fn node_type(class: &str) -> u8 {
    match class {
        "Audio/Sink" => 17,
        "Audio/Source" => 9,
        "Audio/Duplex" => 25,
        "Stream/Output/Audio" => 21,
        "Stream/Input/Audio" => 13,
        "Video/Sink" => 18,
        "Video/Source" => 10,
        _ => 0,
    }
}

fn avg(v: &[f32]) -> f64 {
    if v.is_empty() {
        0.0
    } else {
        v.iter().map(|x| *x as f64).sum::<f64>() / v.len() as f64
    }
}

fn nums(v: Option<Value>) -> Option<Vec<f32>> {
    let Some(Value::Obj(o)) = v else { return None };
    let o = o.borrow();
    Some(
        o.array
            .as_ref()?
            .iter()
            .map(|x| x.to_number() as f32)
            .collect(),
    )
}

fn volumes_value(v: &[f32]) -> Value {
    Value::array(v.iter().map(|x| Value::Num(*x as f64)).collect())
}

fn differs(a: &[f32], b: &[f32]) -> bool {
    a.len() != b.len() || a.iter().zip(b).any(|(x, y)| (x - y).abs() > 1e-4)
}

static GLOBAL: std::sync::OnceLock<Shared> = std::sync::OnceLock::new();
/// Bumped (and waiters woken) after every snapshot write, for [`wait_snapshot`].
static PUBLISHED: (Mutex<u64>, Condvar) = (Mutex::new(0), Condvar::new());

/// The properties of every PipeWire context the shell makes. Without them `client.conf` loads
/// `libpipewire-module-rt` into the shell, which asks the desktop portal (or RTKit) for the
/// realtime budget of a process and sets the answer as the shell's `RLIMIT_RTTIME`. A portal
/// with no RTKit behind it answers 0, and with that limit the kernel kills the whole shell
/// (SIGKILL, nothing logged) when a CPU-clock timer of one of its threads expires, or at the
/// first timer tick a thread spends in a realtime class. The shell needs no realtime thread
/// and keeps no such timer (`watchdog`), a library it loads may: `module.rt` keeps the
/// module out where `client.conf` asks for it by that name, `support.dbus` keeps one loaded
/// by another configuration away from the portal and RTKit.
#[cfg(feature = "pipewire")]
pub(crate) fn context_properties() -> pipewire::properties::PropertiesBox {
    pipewire::properties::properties! {
        "module.rt" => "false",
        "support.dbus" => "false",
    }
}

/// The node name in a `default.*` metadata value (`{"name":"..."}`), JSON-decoded: the
/// first-quote scan this replaced cut names with escaped quotes or backslashes short.
#[cfg(any(feature = "pipewire", test))]
pub(crate) fn json_name(v: &str) -> String {
    serde_json::from_str::<serde_json::Value>(v)
        .ok()
        .and_then(|j| j.get("name")?.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// The channel positions of a node with `n` channels that sent no channel map, as Quickshell
/// (`PwNodeBoundAudio`, after spa/param/audio/layout.h): SPA_AUDIO_CHANNEL ids for 1 to 8
/// channels, none for any other count.
#[cfg(any(feature = "pipewire", test))]
pub(crate) fn default_layout(n: usize) -> Vec<u32> {
    // MONO 2, FL 3, FR 4, FC 5, LFE 6, SL 7, SR 8, RL 12, RR 13.
    match n {
        1 => vec![2],
        2 => vec![3, 4],
        3 => vec![3, 4, 6],
        4 => vec![3, 4, 12, 13],
        5 => vec![3, 4, 5, 7, 8],
        6 => vec![3, 4, 5, 6, 7, 8],
        7 => vec![3, 4, 5, 12, 13, 7, 8],
        8 => vec![3, 4, 5, 6, 12, 13, 7, 8],
        _ => vec![],
    }
}

/// A `default.configured.*` metadata value naming `name`, JSON-encoded so any node name
/// round-trips (it was spliced into a format string unescaped).
#[cfg(any(feature = "pipewire", test))]
pub(crate) fn default_json(name: &str) -> String {
    serde_json::json!({ "name": name }).to_string()
}

/// The latest PipeWire snapshot of the first client, for helper threads (none before the
/// service started).
pub fn snapshot() -> Option<Snap> {
    GLOBAL.get()?.0.lock().unwrap().clone()
}

/// The first published [`snapshot`]: the complete initial state of the daemon, or the empty
/// disconnected one when none could be reached. Starts a client of its own when no runtime
/// has (as `pactl` connects by itself).
pub fn wait_snapshot() -> Snap {
    wait_snapshot_until(|_| true)
}

/// [`wait_snapshot`] for a Process job: `None` once `stop` is set, which ends the wait even
/// when no snapshot is ever published (a daemon that accepted the connection but never
/// answers the initial roundtrips).
pub fn wait_snapshot_or_stop(stop: &Arc<crate::svc::Stop>) -> Option<Snap> {
    start_detached();
    let _wake = stop.on_stop(notify_published);
    wait_published(|| {
        if stop.load(Ordering::Acquire) {
            Some(None)
        } else {
            snapshot().map(Some)
        }
    })
}

/// The first published [`snapshot`] (current or later) that `done` accepts.
pub fn wait_snapshot_until(mut done: impl FnMut(&Snap) -> bool) -> Snap {
    start_detached();
    wait_published(|| snapshot().filter(|s| done(s)))
}

/// Checks `get` once now and once per snapshot write (see [`notify_published`]) until it
/// answers.
fn wait_published<T>(mut get: impl FnMut() -> Option<T>) -> T {
    let mut pubs = PUBLISHED.0.lock().unwrap();
    loop {
        if let Some(v) = get() {
            return v;
        }
        let seen = *pubs;
        pubs = PUBLISHED.1.wait_while(pubs, |g| *g == seen).unwrap();
    }
}

fn notify_published() {
    *PUBLISHED.0.lock().unwrap() += 1;
    PUBLISHED.1.notify_all();
}

/// Stores `snap` as the latest snapshot and wakes the runtime and [`wait_snapshot`] callers.
fn set_snap(shared: &Shared, snap: Snap) {
    *shared.0.lock().unwrap() = Some(snap);
    if !shared.1.swap(true, Ordering::AcqRel) {
        shared.2.wake();
    }
    notify_published();
}

/// Starts the PipeWire client without a runtime (tests and benches of helper threads).
#[cfg(test)]
mod json_tests {
    #[test]
    fn default_node_json_round_trips_any_name() {
        assert_eq!(
            super::json_name(r#"{ "name": "alsa_output.pci-0000_00_1f.3" }"#),
            "alsa_output.pci-0000_00_1f.3"
        );
        for name in ["a\"b", "c\\d", "üñí", ""] {
            assert_eq!(super::json_name(&super::default_json(name)), name);
        }
        assert_eq!(super::json_name("{\"name\":\"x\\\"y\"}"), "x\"y");
        assert_eq!(super::json_name("not json"), "");
        assert_eq!(super::json_name(r#"{"other":"x"}"#), "");
    }
}

/// Starts the first client without a runtime unless one runs already (helper threads, tests).
pub(crate) fn start_detached() {
    GLOBAL.get_or_init(|| {
        let waker = crate::svc::Waker::default();
        let shared = new_shared(waker.handle(crate::svc::Svc::Pipewire));
        drop(start(shared.clone()));
        shared
    });
}

fn new_shared(h: crate::svc::Handle) -> Shared {
    Arc::new((Mutex::new(None), AtomicBool::new(false), h))
}

/// Starts the backend client publishing into `shared`; returns how to send it commands.
fn start(shared: Shared) -> Box<dyn Fn(Cmd)> {
    #[cfg(feature = "pipewire")]
    return backend::start(shared);
    #[cfg(not(feature = "pipewire"))]
    {
        // Built without PipeWire: no daemon can be reached.
        set_snap(&shared, Snap::default());
        Box::new(|_| {})
    }
}

impl Pw {
    fn new(h: crate::svc::Handle) -> Pw {
        let shared = new_shared(h);
        let _ = GLOBAL.set(shared.clone());
        let send = start(shared.clone());
        Pw {
            shared,
            send,
            nodes: HashMap::new(),
            conf: [None, None],
            conf_write: [0; 2],
            writes: 0,
            unready: vec![],
            seq: 0,
        }
    }

    /// QML writes (`sink.audio.volume = x`, `muted`, `preferredDefaultAudioSink`) → PipeWire.
    /// Forwards QML writes on the `dirty` objects to PipeWire.
    fn push_writes(&mut self, w: &mut World, root: usize, dirty: &[usize]) -> bool {
        let mut changed = false;
        let writes = &mut self.writes;
        let mut next_write = || {
            *writes += 1;
            *writes
        };
        for (&id, n) in self.nodes.iter_mut() {
            let Some(a) = n.audio else { continue };
            if !dirty.contains(&a) || w.dead.contains(&a) {
                continue;
            }
            let muted = w.read(a, "muted").is_some_and(|v| v.truthy());
            if muted != n.muted {
                n.muted = muted;
                n.muted_write = next_write();
                (self.send)(Cmd::Muted {
                    id,
                    key: n.snap.key,
                    write: n.muted_write,
                    muted,
                });
            }
            let mut next = None;
            let asked = nums(w.read(a, "volumes")).filter(|v| differs(v, &n.volumes));
            if let Some(v) = asked.as_ref().filter(|v| v.len() != n.volumes.len()) {
                // PwNodeBoundAudio::setVolumes refuses another channel count; the property
                // keeps the node's channels.
                elog::eprintln!(
                    "Tried to change node volumes for PwNode({id}, {:?}) from {:?} to {v:?} \
                     which has a different length than the list of channels {:?}",
                    n.snap.name,
                    n.volumes,
                    n.snap.channels
                );
                w.write(a, "volumes", volumes_value(&n.volumes));
            }
            if let Some(v) = asked.filter(|v| v.len() == n.volumes.len()) {
                next = Some(v);
            } else {
                let vol = w.read(a, "volume").map_or(0.0, |v| v.to_number());
                if (vol - n.vol).abs() > 1e-6 && !n.volumes.is_empty() {
                    // PwNodeBoundAudio::setAverageVolume
                    let old = avg(&n.volumes);
                    let mul = if old == 0.0 { 0.0 } else { vol / old };
                    next = Some(
                        n.volumes
                            .iter()
                            .map(|o| {
                                if mul == 0.0 {
                                    vol as f32
                                } else {
                                    (*o as f64 * mul) as f32
                                }
                            })
                            .collect(),
                    );
                }
            }
            if let Some(v) = next {
                // PwNodeBoundAudio::setVolumes: a negative channel is 0, nothing limits it above.
                let v: Vec<f32> = v.into_iter().map(|x| x.max(0.0)).collect();
                n.volumes = v.clone();
                n.vol = avg(&v);
                w.write(a, "volumes", volumes_value(&v));
                w.write(a, "volume", Value::Num(n.vol));
                n.volumes_write = next_write();
                (self.send)(Cmd::Volumes {
                    id,
                    key: n.snap.key,
                    write: n.volumes_write,
                    volumes: v,
                });
                changed = true;
            }
        }
        for (key, prop) in ["preferredDefaultAudioSink", "preferredDefaultAudioSource"]
            .into_iter()
            .enumerate()
            .filter(|_| dirty.contains(&root))
        {
            let cur = match w.read(root, prop) {
                Some(Value::Item(i)) => Some(i),
                _ => None,
            };
            if cur != self.conf[key] {
                let node = cur
                    .and_then(|i| self.nodes.values().find(|n| n.item == i))
                    .map(|n| (n.snap.ty, n.snap.name.clone()));
                // PwNodeType::AudioSink / AudioSource (Audio | Sink, Audio | Source).
                let (flags, what) = [(17, "sink"), (9, "source")][key];
                match (cur, node) {
                    // As PwDefaultTracker::changeConfiguredSink: refused, the default stays.
                    (_, Some((ty, _))) if ty & flags != flags => {
                        elog::eprintln!(
                            "Cannot change default {what} to a node that is not a {what}."
                        );
                        let was = self.conf[key].map_or(Value::Null, Value::Item);
                        w.write(root, prop, was);
                    }
                    // Not a node of this service: nothing to name.
                    (Some(_), None) => self.conf[key] = cur,
                    // A node, or null, which clears the key.
                    (_, node) => {
                        self.conf[key] = cur;
                        self.conf_write[key] = next_write();
                        (self.send)(Cmd::Default {
                            slot: key,
                            write: self.conf_write[key],
                            name: node.map(|(_, name)| name),
                        });
                    }
                }
            }
        }
        changed
    }

    /// Toggles the mute of the default source, as `wpctl set-mute @DEFAULT_SOURCE@ toggle`
    /// does: also of one quickshell gives no `audio` (media.class `Audio/Source/Virtual`),
    /// which QML cannot write.
    fn toggle_default_source_mute(&mut self, w: &mut World, root: usize) {
        let Some(Value::Item(item)) = w.read(root, "defaultAudioSource") else {
            return;
        };
        let Some((&id, n)) = self.nodes.iter_mut().find(|(_, n)| n.item == item) else {
            return;
        };
        n.muted = !n.muted;
        self.writes += 1;
        n.muted_write = self.writes;
        if let Some(a) = n.audio {
            let writer = w.native_writer.replace(crate::svc::Svc::Pipewire);
            w.write(a, "muted", Value::Bool(n.muted));
            w.native_writer = writer;
        }
        (self.send)(Cmd::Muted {
            id,
            key: n.snap.key,
            write: n.muted_write,
            muted: n.muted,
        });
    }

    fn remove(&mut self, w: &mut World, id: u32) {
        let Some(n) = self.nodes.remove(&id) else {
            return;
        };
        if let Some(a) = n.audio {
            w.destroy(a);
        }
        w.destroy(n.item);
    }

    fn apply(&mut self, w: &mut World, root: usize, s: Snap) {
        let mut seen = Vec::with_capacity(s.nodes.len());
        for ns in s.nodes {
            seen.push(ns.id);
            // Another node under a reused id (in this daemon or a restarted one): the old one is
            // gone (its objects destroyed, as on its removal) and this one is new, appended with
            // `ready` false.
            if self
                .nodes
                .get(&ns.id)
                .is_some_and(|n| n.snap.id != u32::MAX && n.snap.key != ns.key)
            {
                self.remove(w, ns.id);
            }
            let seq = &mut self.seq;
            let n = self.nodes.entry(ns.id).or_insert_with(|| {
                *seq += 1;
                let item = crate::types::native_singleton(w, "PwNode");
                Node {
                    item,
                    audio: None,
                    snap: Arc::new(NodeSnap {
                        id: u32::MAX,
                        ..Default::default()
                    }),
                    vol: -1.0,
                    muted: false,
                    volumes: vec![],
                    volumes_write: 0,
                    muted_write: 0,
                    seq: *seq,
                }
            });
            if Arc::ptr_eq(&n.snap, &ns) || n.snap == ns {
                continue;
            }
            if n.audio.is_none() && ns.ty & 1 != 0 {
                let a = crate::types::native_singleton(w, "PwNodeAudio");
                w.write_watch.insert(a, crate::svc::Svc::Pipewire);
                w.mirror(a, crate::svc::Svc::Pipewire);
                w.write(n.item, "audio", Value::Item(a));
                n.audio = Some(a);
                n.vol = -1.0;
                n.muted = false;
            }
            let i = n.item;
            if n.snap.id != ns.id
                || n.snap.name != ns.name
                || n.snap.props != ns.props
                || n.snap.ty != ns.ty
            {
                let props = new_obj();
                for (k, v) in &ns.props {
                    set_own(&props, k, Value::str(v));
                }
                for (k, v) in [
                    ("id", Value::Num(ns.id as f64)),
                    ("name", Value::str(&ns.name)),
                    ("description", Value::str(&ns.desc)),
                    ("nickname", Value::str(&ns.nick)),
                    ("type", Value::Num(ns.ty as f64)),
                    ("isSink", Value::Bool(ns.ty & 16 != 0)),
                    ("isStream", Value::Bool(ns.ty & 4 != 0)),
                    ("properties", Value::Obj(props)),
                ] {
                    w.write(i, k, v);
                }
            }
            if ns.ready && !n.snap.ready {
                self.unready.push(i);
            }
            if let Some(a) = n.audio {
                // A snapshot from before the server reported the last QML write keeps that
                // write's value: a stale echo would undo it (and the next relative step).
                if ns.volumes_ack >= n.volumes_write
                    && (differs(&ns.volumes, &n.volumes) || n.vol < 0.0)
                {
                    n.volumes = ns.volumes.clone();
                    n.vol = avg(&ns.volumes);
                    w.write(a, "volumes", volumes_value(&ns.volumes));
                    w.write(a, "volume", Value::Num(n.vol));
                }
                if ns.channels != n.snap.channels {
                    w.write(
                        a,
                        "channels",
                        Value::array(ns.channels.iter().map(|x| Value::Num(*x as f64)).collect()),
                    );
                }
                if ns.muted_ack >= n.muted_write && (ns.muted != n.muted || n.snap.id == u32::MAX) {
                    n.muted = ns.muted;
                    w.write(a, "muted", Value::Bool(ns.muted));
                }
            } else if ns.muted_ack >= n.muted_write {
                // Kept for `toggle_default_source_mute`.
                n.muted = ns.muted;
            }
            n.snap = ns;
        }
        let gone: Vec<u32> = self
            .nodes
            .keys()
            .filter(|k| !seen.contains(k))
            .cloned()
            .collect();
        for id in gone {
            self.remove(w, id);
        }
        let mut ids: Vec<&Node> = self.nodes.values().collect();
        ids.sort_by_key(|n| n.seq);
        let ids: Vec<usize> = ids.iter().map(|n| n.item).collect();
        crate::upower::set_model(w, root, "nodes", &ids);
        let find = |name: &str| {
            self.nodes
                .values()
                .find(|n| !name.is_empty() && n.snap.name == name)
                .map_or(Value::Null, |n| Value::Item(n.item))
        };
        let [sink, source, csink, csource] = s.defaults.each_ref().map(|d| find(d));
        for (k, v) in [csink, csource].into_iter().enumerate() {
            // Same for a configured default QML set: kept until the server reported it, unless
            // its node is gone.
            let keep = s.configured_ack[k] < self.conf_write[k]
                && self.conf[k].is_none_or(|i| self.nodes.values().any(|n| n.item == i));
            if !keep {
                self.conf[k] = match v {
                    Value::Item(i) => Some(i),
                    _ => None,
                };
            }
        }
        let [csink, csource] = self.conf.map(|c| c.map_or(Value::Null, Value::Item));
        for (k, v) in [
            ("defaultAudioSink", sink),
            ("defaultAudioSource", source),
            ("preferredDefaultAudioSink", csink),
            ("preferredDefaultAudioSource", csource),
            ("ready", Value::Bool(s.connected)),
        ] {
            w.write(root, k, v);
        }
    }
}

impl Runtime {
    /// Drive the `Pipewire` singleton (if instantiated); returns whether anything changed.
    pub(crate) fn poll_pipewire(&mut self) -> bool {
        let Some(&root) = self.world.borrow().singletons.get("Pipewire") else {
            return false;
        };
        let mut w = self.world.borrow_mut();
        w.mirror(root, crate::svc::Svc::Pipewire);
        let mut dirty = w.take_watched(crate::svc::Svc::Pipewire);
        let mut m = w.pw.take().unwrap_or_else(|| {
            w.write_watch.insert(root, crate::svc::Svc::Pipewire);
            dirty.push(root);
            Pw::new(w.waker.handle(crate::svc::Svc::Pipewire))
        });
        w.native_writer = Some(crate::svc::Svc::Pipewire);
        let mut changed = !dirty.is_empty() && m.push_writes(&mut w, root, &dirty);
        // PwNode::onCoreSync, a dispatch after the snapshot that carried the volumes.
        for i in std::mem::take(&mut m.unready) {
            if !w.dead.contains(&i) {
                w.write(i, "ready", Value::Bool(true));
                changed = true;
            }
        }
        let snap = if m.shared.1.swap(false, Ordering::AcqRel) {
            m.shared.0.lock().unwrap().clone()
        } else {
            None
        };
        if let Some(s) = snap {
            if crate::log_on!("ri::pw") {
                tracing::debug!(
                    target: "ri::pw",
                    "pw: {} nodes defaults {:?} model {:?}",
                    s.nodes.len(),
                    s.defaults,
                    w.read(root, "nodes")
                );
                for n in &s.nodes {
                    tracing::debug!(
                        target: "ri::pw",
                        "pw:  {} {} ty{} vol{:?} mute{}",
                        n.id, n.name, n.ty, n.volumes, n.muted
                    );
                }
            }
            m.apply(&mut w, root, s);
            changed = true;
            if !m.unready.is_empty() {
                w.waker.wake(crate::svc::Svc::Pipewire);
            }
        }
        w.native_writer = None;
        w.pw = Some(m);
        changed
    }
}

/// The daemon socket libpipewire connects to: `PIPEWIRE_REMOTE` (default `pipewire-0`) in
/// `PIPEWIRE_RUNTIME_DIR`, `XDG_RUNTIME_DIR` or `USERPROFILE`. `None` for a remote list.
#[cfg_attr(not(feature = "pipewire"), allow(dead_code))]
pub(crate) fn socket_path() -> Option<std::path::PathBuf> {
    let remote = std::env::var("PIPEWIRE_REMOTE").unwrap_or_else(|_| "pipewire-0".into());
    socket_path_of(&remote, |k| std::env::var_os(k))
}

fn socket_path_of(
    remote: &str,
    var: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> Option<std::path::PathBuf> {
    if remote.is_empty() || remote.contains([',', '[']) {
        return None;
    }
    if remote.starts_with('/') {
        return Some(remote.into());
    }
    let dir = ["PIPEWIRE_RUNTIME_DIR", "XDG_RUNTIME_DIR", "USERPROFILE"]
        .into_iter()
        .find_map(|k| var(k).filter(|v| !v.is_empty()))?;
    Some(std::path::PathBuf::from(dir).join(remote))
}

/// Watches the daemon socket. A self-started PipeWire announces nothing after listen(2) on it,
/// but by default (module-protocol-native `create_servers`) it locks, binds and listens on
/// CORENAME, then does the same for CORENAME-manager: that socket appearing means CORENAME
/// accepts. A daemon from a systemd socket unit is announced by the unit's touch.
#[cfg_attr(not(feature = "pipewire"), allow(dead_code))]
pub(crate) fn watch(socket: &std::path::Path) -> Option<crate::sysbus::SocketWatch> {
    let mut manager = socket.file_name()?.to_owned();
    manager.push("-manager");
    Some(crate::sysbus::SocketWatch::new(socket)?.also(manager))
}

/// Runs `attempt` (`Continue(true)` once the daemon had answered on its connection) until it
/// breaks. After a session it retries at once: a socket-activated daemon keeps its socket
/// across a restart and is started by that connect. A failed attempt (no connection, or one
/// the server closed without an answer) blocks in `wait` until the next socket event
/// ([`crate::sysbus::SocketWatch`]: created, moved in, or touched once listening), and ends
/// for good when no event can come (no watchable socket: one attempt, as `sysbus` makes).
#[cfg(any(feature = "pipewire", test))]
pub(crate) fn supervise(
    mut attempt: impl FnMut() -> std::ops::ControlFlow<(), bool>,
    mut wait: impl FnMut() -> bool,
) {
    loop {
        let std::ops::ControlFlow::Continue(connected) = attempt() else {
            return;
        };
        if !connected && !wait() {
            return;
        }
    }
}

#[cfg(test)]
mod supervise_tests {
    use super::*;
    use std::cell::RefCell;
    use std::ops::ControlFlow;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::sync::mpsc;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ri-pw-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Runs `supervise` with attempts answered by `attempts` (then stop) and socket events by
    /// `events` (then none can come); returns what it did, in order.
    fn trace(attempts: &[bool], events: &[bool]) -> Vec<&'static str> {
        let log = RefCell::new(Vec::new());
        let (mut a, mut e) = (attempts.iter(), events.iter());
        supervise(
            || {
                log.borrow_mut().push("attempt");
                a.next()
                    .map_or(ControlFlow::Break(()), |&c| ControlFlow::Continue(c))
            },
            || {
                log.borrow_mut().push("wait");
                e.next().copied().unwrap_or(false)
            },
        );
        log.into_inner()
    }

    #[test]
    fn socket_path_follows_libpipewire() {
        let env = |k: &str| match k {
            "XDG_RUNTIME_DIR" => Some("/run/user/1000".into()),
            _ => None,
        };
        assert_eq!(
            socket_path_of("pipewire-0", env),
            Some("/run/user/1000/pipewire-0".into())
        );
        assert_eq!(socket_path_of("/tmp/pw", env), Some("/tmp/pw".into()));
        assert_eq!(socket_path_of("a,b", env), None);
        assert_eq!(socket_path_of("pipewire-0", |_| None), None);
    }

    /// The daemon went away after a session (restart): the next attempt follows at once.
    #[test]
    fn a_session_that_ended_is_retried_at_once() {
        assert_eq!(trace(&[true, true], &[]), ["attempt", "attempt", "attempt"]);
    }

    /// A failed attempt (refused, or no socket) is retried on the next socket event only.
    #[test]
    fn a_failed_attempt_waits_for_the_next_socket_event() {
        assert_eq!(
            trace(&[true, false, false], &[true, true]),
            ["attempt", "attempt", "wait", "attempt", "wait", "attempt"]
        );
    }

    /// Without a watchable socket no event can come: one failed attempt is final, with no
    /// blind retries.
    #[test]
    fn without_socket_events_a_failed_attempt_is_final() {
        assert_eq!(trace(&[false, false], &[]), ["attempt", "wait"]);
    }

    /// With the real watch, the attempt after a failed one follows a listening socket being
    /// put at the path (bound and listening at a scratch name, then renamed onto it).
    #[test]
    fn reconnects_when_the_socket_appears() {
        let d = tmp("appear");
        let sock = d.join("pipewire-0");
        let mut watch = crate::sysbus::SocketWatch::new(&sock);
        let (tx, rx) = mpsc::channel();
        let s = sock.clone();
        let h = std::thread::spawn(move || {
            supervise(
                || {
                    let ok = UnixStream::connect(&s).is_ok();
                    tx.send(ok).unwrap();
                    if ok {
                        ControlFlow::Break(())
                    } else {
                        ControlFlow::Continue(false)
                    }
                },
                || watch.as_mut().is_some_and(crate::sysbus::SocketWatch::wait),
            );
        });
        let next = || rx.recv().expect("no attempt");
        assert!(!next(), "connected with no socket");
        let scratch = d.join("pipewire-0.new");
        let _l = UnixListener::bind(&scratch).unwrap();
        std::fs::rename(&scratch, &sock).unwrap();
        assert!(next(), "the attempt after the socket appeared failed");
        h.join().unwrap();
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A self-started daemon refused the connect made on its socket's bind(2); the manager
    /// socket it creates after listening on the first one wakes the watch, and the connect
    /// then succeeds.
    #[test]
    fn a_self_started_daemon_is_reached_once_its_manager_socket_appears() {
        let d = tmp("manager");
        let sock = d.join("pipewire-0");
        let mut w = watch(&sock).unwrap();
        let fd = crate::sysbus::bound_unlistened(&sock);
        assert!(w.wait(), "no event for the socket's creation");
        assert_eq!(
            UnixStream::connect(&sock).unwrap_err().kind(),
            std::io::ErrorKind::ConnectionRefused
        );
        let raw = std::os::fd::AsRawFd::as_raw_fd(&fd);
        // SAFETY: listen(2) on the open socket `fd`.
        let r = unsafe { libc::listen(raw, 8) };
        assert_eq!(r, 0);
        let _manager = crate::sysbus::bound_unlistened(&d.join("pipewire-0-manager"));
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || tx.send(w.wait()));
        assert!(rx.recv().expect("no event for the manager socket"));
        UnixStream::connect(&sock).unwrap();
        let _ = std::fs::remove_dir_all(&d);
    }
}

#[cfg(feature = "pipewire")]
mod backend {
    use super::{Cmd, NodeSnap, Shared, Snap, node_type};
    use pipewire as pw;
    use pw::spa;
    use pw::types::ObjectType;
    use spa::param::{ParamInfoFlags, ParamType};
    use spa::pod::deserialize::PodDeserializer;
    use spa::pod::serialize::PodSerializer;
    use spa::pod::{Object, Pod, Property, Value as PV, ValueArray};
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    use std::rc::{Rc, Weak};
    use std::sync::{Arc, Mutex};

    const PROP_MUTE: u32 = 0x10004;
    const PROP_CV: u32 = 0x10008;
    const PROP_MAP: u32 = 0x1000b;
    const ROUTE_INDEX: u32 = 1;
    const ROUTE_DEVICE: u32 = 3;
    const ROUTE_PROPS: u32 = 10;
    const ROUTE_SAVE: u32 = 13;
    const T_PROPS: u32 = 0x40002;
    const T_ROUTE: u32 = 0x40009;
    const P_PROPS: u32 = 2;
    const P_ROUTE: u32 = 13;

    #[derive(Default, Clone)]
    struct Vol {
        volumes: Vec<f32>,
        channels: Vec<u32>,
        muted: bool,
    }

    struct N {
        proxy: pw::node::Node,
        _l: Option<pw::node::NodeListener>,
        snap: NodeSnap,
        device: Option<u32>,
        route_dev: i32,
        vol: Option<Vol>,
        seq: u64,
        /// The pending initial core sync (`PwNode::syncSeq`), then `ready` once it is done.
        sync: Option<i32>,
        ready: bool,
        /// What `publish` last sent for this node; `None` once anything it shows changed.
        out: Option<Arc<NodeSnap>>,
        /// `SPA_PARAM_INFO_SERIAL` of Props when it was last enumerated.
        props_serial: Option<bool>,
    }

    struct D {
        proxy: pw::device::Device,
        _l: Option<pw::device::DeviceListener>,
        routes: HashMap<i32, (i32, Vol)>,
        /// `SPA_PARAM_INFO_SERIAL` of Route when it was last enumerated.
        route_serial: Option<bool>,
    }

    #[derive(Default)]
    struct St {
        nodes: HashMap<u32, N>,
        devs: HashMap<u32, D>,
        meta: Option<(pw::metadata::Metadata, pw::metadata::MetadataListener)>,
        defaults: [String; 4],
        configured_ack: [u64; 2],
        seq: u64,
        /// Requests whose replies belong to the initial state (binds, enumerations, node
        /// syncs), counted as they are made.
        issued: u64,
        /// The pending core sync of the initial roundtrip and `issued` when it was made.
        init: Option<(i32, u64)>,
        /// The initial state is complete: snapshots are published from then on.
        synced: bool,
        /// Write ids every write of this connection is above: those before it went to an
        /// earlier daemon (or nowhere), and count as acked.
        base: u64,
        /// Core syncs that ack a write, by sync seq; `true` for the second one.
        acks: HashMap<i32, (Ack, u64, bool)>,
    }

    /// What a write changed, to ack it on.
    #[derive(Clone, Copy)]
    enum Ack {
        Volumes(u32, u64),
        Muted(u32, u64),
        Configured(usize),
    }

    fn props_vol(o: &Object) -> Vol {
        let mut r = Vol::default();
        for p in &o.properties {
            match (p.key, &p.value) {
                // rustshell: visual volume = cbrt(linear)
                (PROP_CV, PV::ValueArray(ValueArray::Float(v))) => {
                    r.volumes = v.iter().map(|x| x.cbrt()).collect()
                }
                (PROP_MAP, PV::ValueArray(ValueArray::Id(v))) => {
                    r.channels = v.iter().map(|i| i.0).collect()
                }
                (PROP_MUTE, PV::Bool(b)) => r.muted = *b,
                _ => {}
            }
        }
        if r.channels.is_empty() {
            r.channels = super::default_layout(r.volumes.len());
        }
        r
    }

    fn object(pod: &Pod) -> Option<Object> {
        match PodDeserializer::deserialize_any_from(pod.as_bytes())
            .ok()?
            .1
        {
            PV::Object(o) => Some(o),
            _ => None,
        }
    }

    fn bytes(o: Object) -> Option<Vec<u8>> {
        Some(
            PodSerializer::serialize(std::io::Cursor::new(Vec::new()), &PV::Object(o))
                .ok()?
                .0
                .into_inner(),
        )
    }

    /// Rebuilds only the nodes whose `out` was invalidated; the rest are shared as is.
    fn publish(st: &mut St, shared: &Shared) {
        if !st.synced {
            return;
        }
        let St { nodes, devs, .. } = st;
        let mut out: Vec<(u64, Arc<NodeSnap>)> = nodes
            .values_mut()
            .map(|n| {
                let s = n.out.get_or_insert_with(|| {
                    let mut s = n.snap.clone();
                    let route = n
                        .device
                        .filter(|_| n.route_dev != -1)
                        .and_then(|d| devs.get(&d))
                        .and_then(|d| d.routes.get(&n.route_dev));
                    if let Some(v) = route.map(|r| &r.1).or(n.vol.as_ref()) {
                        s.volumes = v.volumes.clone();
                        s.channels = v.channels.clone();
                        s.muted = v.muted;
                    }
                    s.ready = n.ready;
                    Arc::new(s)
                });
                (n.seq, s.clone())
            })
            .collect();
        // Registry order, which new nodes keep in `Pipewire.nodes`.
        out.sort_by_key(|n| n.0);
        super::set_snap(
            shared,
            Snap {
                nodes: out.into_iter().map(|n| n.1).collect(),
                defaults: st.defaults.clone(),
                connected: true,
                configured_ack: st.configured_ack,
            },
        );
    }

    /// Write `write` on `ack` is reflected in the server's state as far as this client knows
    /// it: the snapshot may show what the server reported instead of the written value.
    fn settle(st: &mut St, ack: Ack, write: u64) {
        fn node(st: &mut St, id: u32, key: u64) -> Option<&mut NodeSnap> {
            let n = st.nodes.get_mut(&id).filter(|n| n.snap.key == key)?;
            n.out = None;
            Some(&mut n.snap)
        }
        match ack {
            Ack::Volumes(id, key) => {
                if let Some(s) = node(st, id, key) {
                    s.volumes_ack = s.volumes_ack.max(write);
                }
            }
            Ack::Muted(id, key) => {
                if let Some(s) = node(st, id, key) {
                    s.muted_ack = s.muted_ack.max(write);
                }
            }
            Ack::Configured(k) => st.configured_ack[k] = st.configured_ack[k].max(write),
        }
    }

    /// Nodes showing a route of device `id` must be rebuilt by the next `publish`.
    fn routes_changed(st: &mut St, id: u32) {
        for n in st.nodes.values_mut().filter(|n| n.device == Some(id)) {
            n.out = None;
        }
    }

    /// Sends `c` to the server; what to ack once it is done (`None`: its node is gone).
    fn cmd(st: &mut St, c: Cmd) -> Option<(Ack, u64)> {
        let route_props = |n: &N, props: Vec<Property>| -> bool {
            let Some(d) = n
                .device
                .filter(|_| n.route_dev != -1)
                .and_then(|d| st.devs.get(&d))
            else {
                return false;
            };
            let Some((index, _)) = d.routes.get(&n.route_dev) else {
                return false;
            };
            let o = Object {
                type_: T_ROUTE,
                id: P_ROUTE,
                properties: vec![
                    Property::new(ROUTE_INDEX, PV::Int(*index)),
                    Property::new(ROUTE_DEVICE, PV::Int(n.route_dev)),
                    Property::new(
                        ROUTE_PROPS,
                        PV::Object(Object {
                            type_: T_PROPS,
                            id: P_ROUTE,
                            properties: props,
                        }),
                    ),
                    Property::new(ROUTE_SAVE, PV::Bool(true)),
                ],
            };
            if let Some(b) = bytes(o)
                && let Some(pod) = Pod::from_bytes(&b)
            {
                d.proxy.set_param(ParamType::Route, 0, pod);
            }
            true
        };
        let node_props = |n: &N, props: Vec<Property>| {
            if let Some(b) = bytes(Object {
                type_: T_PROPS,
                id: P_PROPS,
                properties: props,
            }) && let Some(pod) = Pod::from_bytes(&b)
            {
                n.proxy.set_param(ParamType::Props, 0, pod);
            }
        };
        match c {
            Cmd::Volumes {
                id,
                key,
                write,
                volumes,
            } => {
                let n = st.nodes.get(&id).filter(|n| n.snap.key == key)?;
                let cubed: Vec<f32> = volumes.iter().map(|x| x * x * x).collect();
                let p = || {
                    vec![Property::new(
                        PROP_CV,
                        PV::ValueArray(ValueArray::Float(cubed.clone())),
                    )]
                };
                if !route_props(n, p()) {
                    node_props(n, p());
                }
                Some((Ack::Volumes(id, key), write))
            }
            Cmd::Muted {
                id,
                key,
                write,
                muted,
            } => {
                let n = st.nodes.get_mut(&id).filter(|n| n.snap.key == key)?;
                let p = || vec![Property::new(PROP_MUTE, PV::Bool(muted))];
                if !route_props(n, p()) {
                    node_props(n, p());
                    // PwNodeBoundAudio::setMuted: the node is as written until its Props say
                    // otherwise. The server does not answer every enumeration the write's
                    // info starts, and the ack must not bring the state before the write back.
                    if let Some(v) = &mut n.vol {
                        v.muted = muted;
                        n.out = None;
                    }
                }
                Some((Ack::Muted(id, key), write))
            }
            Cmd::Default { slot, write, name } => {
                if let Some((m, _)) = &st.meta {
                    // No value clears the key, as `pw_metadata_set_property` documents.
                    let value = name.as_deref().map(super::default_json);
                    m.set_property(
                        0,
                        super::CONFIGURED[slot],
                        Some("Spa:String:JSON"),
                        value.as_deref(),
                    );
                }
                Some((Ack::Configured(slot), write))
            }
        }
    }

    /// The runtime's end of the command channel: the current connection's sender, if any, and
    /// the last write id handed to it (or dropped while there was none).
    #[derive(Default)]
    struct Link {
        tx: Option<pw::channel::Sender<Cmd>>,
        last: u64,
    }

    type Sender = Arc<Mutex<Link>>;

    pub fn start(shared: Shared) -> Box<dyn Fn(Cmd)> {
        let tx: Sender = Arc::default();
        let (t, sh) = (tx.clone(), shared.clone());
        let r = std::thread::Builder::new()
            .name("pipewire".into())
            .spawn(move || {
                pw::init();
                // Set up before the first attempt so a socket created in between is not missed.
                let mut watch = super::socket_path().as_deref().and_then(super::watch);
                super::supervise(
                    || attempt(&sh, &t),
                    || watch.as_mut().is_some_and(crate::sysbus::SocketWatch::wait),
                );
            });
        if let Err(e) = r {
            // No client will ever run: the state is that of a daemon nobody can reach.
            elog::eprintln!("pipewire: cannot start the client thread: {e}");
            super::set_snap(&shared, Snap::default());
        }
        Box::new(move |c| {
            let mut l = tx.lock().unwrap();
            l.last = c.write();
            if let Some(s) = l.tx.as_ref() {
                let _ = s.send(c);
            }
        })
    }

    /// One connection, from the connect until the daemon is gone, then the disconnected
    /// snapshot. `Continue(true)` when the daemon answered on it: a socket accepting the
    /// connection is not a daemon yet, and one that closes it at once would be connected to
    /// again without a pause.
    fn attempt(shared: &Shared, tx: &Sender) -> std::ops::ControlFlow<(), bool> {
        let answered = Rc::new(Cell::new(false));
        if let Err(e) = run(shared, tx, &answered) {
            elog::eprintln!("pipewire: {e}");
        }
        // Daemon gone (restart): drop everything, then reconnect. Writes sent so far
        // went with it; the next connection acks them from the start.
        let last = {
            let mut l = tx.lock().unwrap();
            l.tx = None;
            l.last
        };
        super::set_snap(
            shared,
            Snap {
                configured_ack: [last; 2],
                ..Snap::default()
            },
        );
        std::ops::ControlFlow::Continue(answered.get())
    }

    /// `answered` is set by the daemon's first answer to a core sync.
    fn run(shared: &Shared, tx: &Sender, answered: &Rc<Cell<bool>>) -> Result<(), pw::Error> {
        let ml = pw::main_loop::MainLoopRc::new(None)?;
        let ctx = pw::context::ContextRc::new(&ml, Some(super::context_properties()))?;
        let core = ctx.connect_rc(None)?;
        let reg = core.get_registry_rc()?;
        let (s, r) = pw::channel::channel::<Cmd>();
        let base = {
            let mut l = tx.lock().unwrap();
            l.tx = Some(s);
            l.last
        };
        let st = Rc::new(RefCell::new(St {
            configured_ack: [base; 2],
            base,
            ..St::default()
        }));
        let core_w = Rc::new(core.downgrade());
        // A write is acked by a core sync after it, then another after that sync's done. The
        // server answers a sync once it processed every earlier request and sent the events
        // they caused, here the param info change, whose handler enumerates the new Props /
        // Route; the second sync's done follows that enumeration's params.
        let _rx = r.attach(ml.loop_(), {
            let (st, core) = (st.clone(), core_w.clone());
            move |c| {
                let Some((ack, write)) = cmd(&mut st.borrow_mut(), c) else {
                    return;
                };
                if let Some(core) = core.upgrade()
                    && let Ok(seq) = core.sync(0)
                {
                    st.borrow_mut().acks.insert(seq.seq(), (ack, write, false));
                }
            }
        });
        let _core_l = core
            .add_listener_local()
            .done({
                let (st, sh, core) = (Rc::downgrade(&st), shared.clone(), core_w.clone());
                let answered = answered.clone();
                move |id, seq| {
                    let Some(st) = st.upgrade() else { return };
                    if id != pw::core::PW_ID_CORE {
                        return;
                    }
                    answered.set(true);
                    let mut stm = st.borrow_mut();
                    // The initial roundtrip: the server answers requests in order, so this
                    // sync's done follows the replies to every request made before it. Requests
                    // made since (binds of the globals it announced, the enumerations their info
                    // started) take another roundtrip, until one completes with none made.
                    if let Some((init, issued)) = stm.init
                        && init == seq.seq()
                    {
                        if stm.issued == issued {
                            stm.init = None;
                            stm.synced = true;
                            publish(&mut stm, &sh);
                        } else {
                            let next = core.upgrade().and_then(|c| c.sync(0).ok());
                            stm.init = next.map(|s| (s.seq(), stm.issued));
                        }
                        return;
                    }
                    if let Some((ack, write, second)) = stm.acks.remove(&seq.seq()) {
                        if !second
                            && let Some(core) = core.upgrade()
                            && let Ok(next) = core.sync(0)
                        {
                            stm.acks.insert(next.seq(), (ack, write, true));
                        } else {
                            settle(&mut stm, ack, write);
                            publish(&mut stm, &sh);
                        }
                        return;
                    }
                    // PwNode::onCoreSync: the node's initial info and params arrived before this.
                    let Some(n) = stm.nodes.values_mut().find(|n| n.sync == Some(seq.seq())) else {
                        return;
                    };
                    n.sync = None;
                    n.ready = true;
                    n.out = None;
                    publish(&mut stm, &sh);
                }
            })
            .error({
                let ml = ml.downgrade();
                move |id, _seq, res, msg| {
                    elog::eprintln!("pipewire: core error {id} {res} {msg}");
                    if id == 0
                        && let Some(ml) = ml.upgrade()
                    {
                        ml.quit();
                    }
                }
            })
            .register();
        let regw = reg.downgrade();
        let stw = Rc::downgrade(&st);
        let sh = shared.clone();
        let sh2 = shared.clone();
        let stw2 = stw.clone();
        let _reg_l = reg
            .add_listener_local()
            .global(move |g| {
                let (Some(reg), Some(st)) = (regw.upgrade(), stw.upgrade()) else {
                    return;
                };
                match g.type_ {
                    ObjectType::Node => {
                        let Ok(proxy) = reg.bind::<pw::node::Node, _>(g) else {
                            return;
                        };
                        let id = g.id;
                        let l = node_listener(&proxy, id, stw.clone(), sh.clone(), core_w.clone());
                        let mut stm = st.borrow_mut();
                        let mut snap = NodeSnap {
                            id,
                            key: super::node_key(),
                            volumes_ack: stm.base,
                            muted_ack: stm.base,
                            ..Default::default()
                        };
                        if let Some(p) = g.props {
                            snap.ty = node_type(p.get("media.class").unwrap_or(""));
                            snap.serial = p.get("object.serial").and_then(|s| s.parse().ok());
                        }
                        stm.issued += 1;
                        stm.seq += 1;
                        let seq = stm.seq;
                        stm.nodes.insert(
                            id,
                            N {
                                proxy,
                                _l: Some(l),
                                snap,
                                device: None,
                                route_dev: -1,
                                vol: None,
                                seq,
                                sync: None,
                                ready: false,
                                out: None,
                                props_serial: None,
                            },
                        );
                    }
                    ObjectType::Device => {
                        let Ok(proxy) = reg.bind::<pw::device::Device, _>(g) else {
                            return;
                        };
                        let id = g.id;
                        let (w, s) = (stw.clone(), sh.clone());
                        let wi = stw.clone();
                        let l = proxy
                            .add_listener_local()
                            .info(move |info| {
                                // PwDevice::onInfo: re-enumerate Routes whenever their param info
                                // changes (SPA_PARAM_INFO_SERIAL toggles on every change).
                                if info
                                    .change_mask()
                                    .contains(pw::device::DeviceChangeMask::PARAMS)
                                    && let Some(p) = info.params().iter().find(|p| {
                                        p.id() == ParamType::Route
                                            && p.flags().contains(ParamInfoFlags::READ)
                                    })
                                    && let Some(st) = wi.upgrade()
                                {
                                    let mut stm = st.borrow_mut();
                                    let St { devs, issued, .. } = &mut *stm;
                                    let serial = p.flags().contains(ParamInfoFlags::SERIAL);
                                    if let Some(d) = devs.get_mut(&id)
                                        && d.route_serial.replace(serial) != Some(serial)
                                    {
                                        // PwDevice rebuilds its route map from each enumeration,
                                        // so a profile switch drops the routes of the old one.
                                        d.routes.clear();
                                        d.proxy.enum_params(0, Some(ParamType::Route), 0, u32::MAX);
                                        *issued += 1;
                                    }
                                }
                            })
                            .param(move |_, ty, _, _, pod| {
                                if ty != ParamType::Route {
                                    return;
                                }
                                let Some(st) = w.upgrade() else { return };
                                let Some(o) = pod.and_then(object) else {
                                    return;
                                };
                                let (mut index, mut dev, mut vol) = (-1, -1, None);
                                for p in &o.properties {
                                    match (p.key, &p.value) {
                                        (ROUTE_INDEX, PV::Int(i)) => index = *i,
                                        (ROUTE_DEVICE, PV::Int(i)) => dev = *i,
                                        (ROUTE_PROPS, PV::Object(po)) => vol = Some(props_vol(po)),
                                        _ => {}
                                    }
                                }
                                let mut stm = st.borrow_mut();
                                if let (Some(v), Some(d)) = (vol, stm.devs.get_mut(&id)) {
                                    d.routes.insert(dev, (index, v));
                                }
                                routes_changed(&mut stm, id);
                                publish(&mut stm, &s);
                            })
                            .register();
                        let mut stm = st.borrow_mut();
                        stm.issued += 1;
                        stm.devs.insert(
                            id,
                            D {
                                proxy,
                                _l: Some(l),
                                routes: HashMap::new(),
                                route_serial: None,
                            },
                        );
                    }
                    ObjectType::Metadata => {
                        if g.props.and_then(|p| p.get("metadata.name")) != Some("default") {
                            return;
                        }
                        let Ok(m) = reg.bind::<pw::metadata::Metadata, _>(g) else {
                            return;
                        };
                        let (w, s) = (stw.clone(), sh.clone());
                        let l = m
                            .add_listener_local()
                            .property(move |_, key, _, value| {
                                let Some(st) = w.upgrade() else { return 0 };
                                let k = match key {
                                    Some("default.audio.sink") => 0,
                                    Some("default.audio.source") => 1,
                                    Some("default.configured.audio.sink") => 2,
                                    Some("default.configured.audio.source") => 3,
                                    _ => return 0,
                                };
                                let mut stm = st.borrow_mut();
                                stm.defaults[k] = value.map(super::json_name).unwrap_or_default();
                                publish(&mut stm, &s);
                                0
                            })
                            .register();
                        let mut stm = st.borrow_mut();
                        stm.issued += 1;
                        stm.meta = Some((m, l));
                    }
                    _ => {}
                }
            })
            .global_remove(move |id| {
                let Some(st) = stw2.upgrade() else { return };
                let mut stm = st.borrow_mut();
                let a = stm.nodes.remove(&id).is_some();
                let b = stm.devs.remove(&id).is_some();
                if b {
                    routes_changed(&mut stm, id);
                }
                if a || b {
                    publish(&mut stm, &sh2);
                }
            })
            .register();
        let first = core.sync(0)?;
        st.borrow_mut().init = Some((first.seq(), 0));
        ml.run();
        Ok(())
    }

    fn node_listener(
        proxy: &pw::node::Node,
        id: u32,
        w: Weak<RefCell<St>>,
        s: Shared,
        core: Rc<pw::core::CoreWeak>,
    ) -> pw::node::NodeListener {
        let (w2, s2) = (w.clone(), s.clone());
        proxy
            .add_listener_local()
            .info(move |info| {
                let Some(st) = w.upgrade() else { return };
                {
                    let mut stm = st.borrow_mut();
                    let St { nodes, issued, .. } = &mut *stm;
                    let Some(n) = nodes.get_mut(&id) else { return };
                    // PwNodeBoundAudio::onInfo: enumerate Props when its param info changes.
                    // The server toggles SPA_PARAM_INFO_SERIAL on every change, so comparing it
                    // skips info events about other params (Format, Latency, ...).
                    if info
                        .change_mask()
                        .contains(pw::node::NodeChangeMask::PARAMS)
                        && let Some(p) = info.params().iter().find(|p| {
                            p.id() == ParamType::Props
                                && p.flags()
                                    .contains(ParamInfoFlags::READ | ParamInfoFlags::WRITE)
                        })
                    {
                        let serial = p.flags().contains(ParamInfoFlags::SERIAL);
                        if n.props_serial.replace(serial) != Some(serial) {
                            n.proxy.enum_params(0, Some(ParamType::Props), 0, u32::MAX);
                            *issued += 1;
                        }
                    }
                    // PwNode::onInfo: a core sync after the first info (and the enumeration it
                    // started) marks the node ready once its initial volumes have arrived.
                    if !n.ready
                        && n.sync.is_none()
                        && let Some(core) = core.upgrade()
                    {
                        n.sync = core.sync(0).ok().map(|s| s.seq());
                        *issued += 1;
                    }
                }
                if !info.change_mask().contains(pw::node::NodeChangeMask::PROPS) {
                    return;
                }
                let Some(p) = info.props() else { return };
                let mut stm = st.borrow_mut();
                let devs: Vec<u32> = stm.devs.keys().cloned().collect();
                let Some(n) = stm.nodes.get_mut(&id) else {
                    return;
                };
                n.snap.props = p
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect();
                n.snap.name = p.get("node.name").unwrap_or("").to_string();
                n.snap.desc = p.get("node.description").unwrap_or("").to_string();
                n.snap.nick = p.get("node.nick").unwrap_or("").to_string();
                n.snap.ty = node_type(p.get("media.class").unwrap_or(""));
                n.device = p
                    .get("device.id")
                    .and_then(|d| d.parse().ok())
                    .filter(|d| devs.contains(d));
                if n.device.is_some()
                    && let Some(r) = p.get("card.profile.device").and_then(|d| d.parse().ok())
                {
                    n.route_dev = r;
                }
                n.out = None;
                publish(&mut stm, &s);
            })
            .param(move |_, ty, index, _, pod| {
                if ty != ParamType::Props || index != 0 {
                    return;
                }
                let Some(st) = w2.upgrade() else { return };
                let Some(o) = pod.and_then(object) else {
                    return;
                };
                let mut stm = st.borrow_mut();
                let Some(n) = stm.nodes.get_mut(&id) else {
                    return;
                };
                n.vol = Some(props_vol(&o));
                n.out = None;
                publish(&mut stm, &s2);
            })
            .register()
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::ops::ControlFlow;

        /// The client thread refused: the disconnected snapshot is published at once, so
        /// `Pipewire.ready` settles and `wait_snapshot()` returns, and the refusal is in the
        /// log. In a process of its own: the test reads what that process wrote to its stderr.
        #[test]
        fn a_refused_thread_publishes_the_disconnected_snapshot() {
            const TEST: &str = concat!(
                module_path!(),
                "::a_refused_thread_publishes_the_disconnected_snapshot"
            );
            if !crate::testenv::in_own_process(TEST) {
                crate::testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
                return;
            }
            let waker = crate::svc::Waker::default();
            let shared = super::super::new_shared(waker.handle(crate::svc::Svc::Pipewire));
            crate::testenv::refuse_new_threads();
            let text = crate::testenv::stderr_of(|| drop(start(shared.clone())));
            let snap = shared.0.lock().unwrap().clone();
            assert_eq!(snap.map(|s| s.connected), Some(false), "no snapshot");
            assert!(waker.take().has(crate::svc::Svc::Pipewire));
            assert_eq!(
                text.matches("pipewire: cannot start the client thread:")
                    .count(),
                1,
                "stderr: {text:?}"
            );
            // The client `wait_snapshot` starts by itself is refused too: it returns.
            assert!(!super::super::wait_snapshot().connected);
        }

        /// A server that accepts the connection and closes it at once did not answer: the
        /// attempt counts as failed, so the next one waits for a socket event instead of
        /// following at once. In a process of its own, for the socket's place in its
        /// environment.
        #[test]
        fn a_connection_closed_unanswered_is_a_failed_attempt() {
            const TEST: &str = concat!(
                module_path!(),
                "::a_connection_closed_unanswered_is_a_failed_attempt"
            );
            if !crate::testenv::in_own_process(TEST) {
                let dir = tempfile::tempdir().unwrap();
                crate::testenv::run_in_own_process_env(
                    TEST,
                    [("XDG_RUNTIME_DIR", dir.path())],
                    &["PIPEWIRE_REMOTE", "PIPEWIRE_RUNTIME_DIR"],
                );
                return;
            }
            let socket = super::super::socket_path().expect("the socket's path");
            let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
            let (accepted, accepts) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                for c in listener.incoming() {
                    // Counted before the close that ends the client's attempt.
                    if accepted.send(()).is_err() {
                        break;
                    }
                    drop(c);
                }
            });
            let waker = crate::svc::Waker::default();
            let shared = super::super::new_shared(waker.handle(crate::svc::Svc::Pipewire));
            let tx = Sender::default();
            pw::init();
            let trace = RefCell::new(Vec::new());
            super::super::supervise(
                || {
                    // Retried without a pause it would go on for ever: three attempts show it.
                    if trace.borrow().len() == 3 {
                        return ControlFlow::Break(());
                    }
                    trace.borrow_mut().push("attempt");
                    attempt(&shared, &tx)
                },
                || {
                    trace.borrow_mut().push("wait");
                    false
                },
            );
            assert_eq!(trace.into_inner(), ["attempt", "wait"]);
            assert_eq!(accepts.try_iter().count(), 1);
            let snap = shared.0.lock().unwrap().clone();
            assert_eq!(snap.map(|s| s.connected), Some(false), "no snapshot");
        }
    }
}

#[cfg(test)]
mod wait_tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc;

    static READY: AtomicBool = AtomicBool::new(false);

    /// A waiter that checked and found nothing is woken by the publish that answers it: the
    /// check runs under the lock the publish takes, so the publish cannot slip in between.
    #[test]
    fn a_waiter_wakes_on_the_publish_that_answers_it() {
        let (tx, rx) = mpsc::channel();
        let w = std::thread::spawn(move || {
            wait_published(|| {
                let r = READY.load(Ordering::Acquire).then_some(7);
                let _ = tx.send(());
                r
            })
        });
        rx.recv().unwrap();
        READY.store(true, Ordering::Release);
        notify_published();
        assert_eq!(w.join().unwrap(), 7);
    }

    #[test]
    fn a_ready_answer_returns_at_once() {
        assert_eq!(wait_published(|| Some(7)), 7);
    }
}

#[cfg(all(test, feature = "interp", not(feature = "pipewire")))]
mod tests {
    use super::*;
    use crate::runtime::Runtime;

    /// The snapshot slot of this runtime's client (`GLOBAL` is the first runtime's).
    fn own(rt: &Runtime) -> Shared {
        let w = rt.world.borrow();
        w.pw.as_ref().expect("Pipewire started").shared.clone()
    }

    #[test]
    fn new_nodes_append_to_the_model_like_objectmodel() {
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(
                r#"import QtQuick
import Rustshell.Services.Pipewire
Item { readonly property string names: Pipewire.nodes.values.map(n => n.name).join(",") }"#,
            )
            .unwrap();
        rt.settle();
        let node = |id: u32, name: &str| NodeSnap {
            id,
            name: name.into(),
            ty: 21,
            volumes: vec![1.0, 1.0],
            ..Default::default()
        };
        let feed = |rt: &mut Runtime, nodes: Vec<NodeSnap>| {
            rt.poll_native();
            let sh = own(rt);
            set_snap(
                &sh,
                Snap {
                    nodes: nodes.into_iter().map(Arc::new).collect(),
                    ..Default::default()
                },
            );
            rt.poll_native();
            rt.settle();
            rt.poll_native();
            rt.settle();
        };
        feed(&mut rt, vec![node(77, "zed")]);
        // Reused lower ids (75) arrive later than 77: still appended, not sorted in.
        feed(&mut rt, vec![node(75, "alpha"), node(77, "zed")]);
        feed(
            &mut rt,
            vec![node(75, "alpha"), node(77, "zed"), node(89, "mid")],
        );
        assert_eq!(rt.get(root, "names").to_js_string(), "zed,alpha,mid");
    }

    /// The original's OnScreenDisplay shows on a default sink volume change once `ready`: a sink whose
    /// Props arrive after its first info (a PipeWire dispatch apart) must not be ready before
    /// them, or its initial volume pops the volume OSD at startup.
    #[test]
    fn sink_is_not_ready_before_its_initial_volumes() {
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(
                r#"import QtQuick
import Rustshell.Services.Pipewire
Item {
    readonly property var sink: Pipewire.defaultAudioSink
    readonly property bool ready: sink?.ready ?? false
    property int osd: 0
    PwObjectTracker { objects: [sink] }
    Connections {
        target: root.sink?.audio ?? null
        function onVolumeChanged() { if (root.ready) root.osd++; }
    }
    id: root
}"#,
            )
            .unwrap();
        rt.settle();
        let feed = |rt: &mut Runtime, volumes: Vec<f32>, ready: bool| {
            rt.poll_native();
            let sh = own(rt);
            let node = NodeSnap {
                id: 41,
                name: "sink".into(),
                ty: 17,
                volumes,
                channels: vec![3, 4],
                ready,
                ..Default::default()
            };
            set_snap(
                &sh,
                Snap {
                    nodes: vec![Arc::new(node)],
                    defaults: ["sink".into(), String::new(), "sink".into(), String::new()],
                    connected: true,
                    ..Default::default()
                },
            );
            for _ in 0..3 {
                rt.poll_native();
                rt.settle();
            }
        };
        // First info: props, no Props params yet; then the params; then the core sync.
        feed(&mut rt, vec![], false);
        assert!(!rt.get(root, "ready").truthy());
        feed(&mut rt, vec![0.4, 0.4], false);
        assert!(!rt.get(root, "ready").truthy());
        feed(&mut rt, vec![0.4, 0.4], true);
        assert!(rt.get(root, "ready").truthy());
        assert_eq!(
            rt.get(root, "osd").to_number(),
            0.0,
            "startup volume showed the OSD"
        );
        feed(&mut rt, vec![0.6, 0.6], true);
        assert_eq!(
            rt.get(root, "osd").to_number(),
            1.0,
            "a real volume change is shown"
        );
    }

    fn audio_node(id: u32, volumes: Vec<f32>) -> NodeSnap {
        NodeSnap {
            id,
            name: format!("sink{id}"),
            ty: 17,
            volumes,
            channels: vec![3, 4],
            ready: true,
            ..Default::default()
        }
    }

    fn describe(c: &Cmd) -> String {
        match c {
            // Steps of 0.1 from a shown f64 carry float noise (0.70000005): to the thousandth.
            Cmd::Volumes { id, volumes, .. } => {
                let v: Vec<String> = volumes.iter().map(|x| format!("{x:.3}")).collect();
                format!("vol {id} [{}]", v.join(", "))
            }
            Cmd::Muted { id, muted, .. } => format!("mute {id} {muted}"),
            Cmd::Default { slot, name, .. } => {
                let name = name.as_deref().unwrap_or("(cleared)");
                format!("default {} {name}", CONFIGURED[*slot])
            }
        }
    }

    /// Records what the runtime sends from now on, with the write ids.
    fn record(rt: &Runtime) -> std::rc::Rc<std::cell::RefCell<Vec<(String, u64)>>> {
        let sent = std::rc::Rc::new(std::cell::RefCell::new(vec![]));
        let s = sent.clone();
        rt.world.borrow_mut().pw.as_mut().unwrap().send =
            Box::new(move |c| s.borrow_mut().push((describe(&c), c.write())));
        sent
    }

    fn feed(rt: &mut Runtime, snap: Snap) {
        rt.poll_native();
        set_snap(&own(rt), snap);
        rt.poll_native();
        rt.settle();
    }

    /// The `audio` object of the node `root.sink`.
    fn audio_of(rt: &Runtime, root: usize) -> usize {
        let mut w = rt.world.borrow_mut();
        let Some(Value::Item(n)) = w.read(root, "sink") else {
            panic!("no sink")
        };
        let Some(Value::Item(a)) = w.read(n, "audio") else {
            panic!("no audio")
        };
        a
    }

    const SINK_QML: &str = r#"import QtQuick
import Rustshell.Services.Pipewire
Item {
    property var sink: Pipewire.nodes.values[0] ?? null
    PwObjectTracker { objects: [sink] }
}"#;

    /// A node snapshot of `audio_node(41, ..)` with the given acks.
    fn acked(volumes: Vec<f32>, muted: bool, volumes_ack: u64, muted_ack: u64) -> Snap {
        Snap {
            nodes: vec![Arc::new(NodeSnap {
                muted,
                volumes_ack,
                muted_ack,
                ..audio_node(41, volumes)
            })],
            ..Default::default()
        }
    }

    /// Writes `volume` (the average) or `volumes` on node 41 shown with `start`, and returns
    /// what was sent and the shown average and channels.
    fn write_audio(start: Vec<f32>, prop: &str, v: Value) -> (Vec<String>, f64, String) {
        let mut rt = Runtime::new(None);
        let root = rt.load_str(SINK_QML).unwrap();
        rt.settle();
        feed_nodes(&mut rt, vec![audio_node(41, start)]);
        let sent = record(&rt);
        let a = audio_of(&rt, root);
        rt.world.borrow_mut().write(a, prop, v);
        rt.poll_native();
        rt.settle();
        let sent = sent.borrow().iter().map(|x| x.0.clone()).collect();
        let mut w = rt.world.borrow_mut();
        let shown = w.read(a, "volume").unwrap().to_number();
        let chans = nums(w.read(a, "volumes")).unwrap();
        let chans: Vec<String> = chans.iter().map(|x| format!("{x:.3}")).collect();
        (sent, shown, chans.join(", "))
    }

    /// Setting the average of unbalanced channels multiplies every channel by the same
    /// factor, as `PwNodeBoundAudio::setAverageVolume` does; the louder one may pass 100%.
    #[test]
    fn an_average_write_scales_every_channel_alike() {
        let (sent, shown, chans) = write_audio(vec![1.0, 0.5], "volume", Value::Num(1.0));
        assert_eq!(sent, ["vol 41 [1.333, 0.667]"]);
        assert!((shown - 1.0).abs() < 1e-6, "{shown}");
        assert_eq!(chans, "1.333, 0.667");
        let (sent, shown, _) = write_audio(vec![0.8, 0.4], "volume", Value::Num(0.9));
        assert_eq!(sent, ["vol 41 [1.200, 0.600]"]);
        assert!((shown - 0.9).abs() < 1e-6, "{shown}");
        let (sent, _, _) = write_audio(vec![0.8, 0.4], "volume", Value::Num(0.45));
        assert_eq!(sent, ["vol 41 [0.600, 0.300]"]);
    }

    /// A write above 100% goes out as asked: `PwNodeBoundAudio::setVolumes` only turns a
    /// negative channel into 0, and the original's own limit (`Audio.hardMaxValue`, 200%) is in QML.
    #[test]
    fn a_write_above_one_hundred_percent_goes_out_as_asked() {
        let (sent, shown, _) = write_audio(vec![0.5, 0.5], "volume", Value::Num(1.5));
        assert_eq!(sent, ["vol 41 [1.500, 1.500]"]);
        assert_eq!(shown, 1.5);
        let (sent, _, _) = write_audio(vec![0.0, 0.0], "volume", Value::Num(1.5));
        assert_eq!(sent, ["vol 41 [1.500, 1.500]"]);
        let unbalanced = Value::array(vec![Value::Num(1.5), Value::Num(0.75)]);
        let (sent, shown, _) = write_audio(vec![0.5, 0.5], "volumes", unbalanced);
        assert_eq!(sent, ["vol 41 [1.500, 0.750]"]);
        assert_eq!(shown, 1.125);
        let (sent, _, _) = write_audio(vec![0.5, 0.5], "volume", Value::Num(-0.5));
        assert_eq!(sent, ["vol 41 [0.000, 0.000]"]);
    }

    /// A `volumes` write with another channel count is refused, as
    /// `PwNodeBoundAudio::setVolumes` does ("has a different length than the list of
    /// channels"): nothing is sent and the shown channels and average stay the node's
    /// (`[0.5]` on a stereo sink was sent as is).
    #[test]
    fn a_volumes_write_with_another_channel_count_is_refused() {
        let mono = Value::array(vec![Value::Num(0.8)]);
        let (sent, shown, chans) = write_audio(vec![0.5, 0.25], "volumes", mono);
        assert!(sent.is_empty(), "{sent:?}");
        assert_eq!((shown, chans.as_str()), (0.375, "0.500, 0.250"));
        let three = Value::array(vec![Value::Num(0.1), Value::Num(0.2), Value::Num(0.3)]);
        let (sent, shown, chans) = write_audio(vec![0.5, 0.25], "volumes", three);
        assert!(sent.is_empty(), "{sent:?}");
        assert_eq!((shown, chans.as_str()), (0.375, "0.500, 0.250"));
        let (sent, shown, chans) = write_audio(vec![0.5, 0.25], "volumes", Value::array(vec![]));
        assert!(sent.is_empty(), "{sent:?}");
        assert_eq!((shown, chans.as_str()), (0.375, "0.500, 0.250"));
    }

    /// After a refused `volumes` write, a write of the right length still goes out.
    #[test]
    fn a_refused_volumes_write_does_not_block_the_next_one() {
        let mut rt = Runtime::new(None);
        let root = rt.load_str(SINK_QML).unwrap();
        rt.settle();
        feed_nodes(&mut rt, vec![audio_node(41, vec![0.5, 0.5])]);
        let sent = record(&rt);
        let a = audio_of(&rt, root);
        let write = |rt: &mut Runtime, v: Vec<f64>| {
            let v = Value::array(v.into_iter().map(Value::Num).collect());
            rt.world.borrow_mut().write(a, "volumes", v);
            rt.poll_native();
            rt.settle();
        };
        write(&mut rt, vec![0.8]);
        write(&mut rt, vec![0.8, 0.4]);
        let sent: Vec<String> = sent.borrow().iter().map(|x| x.0.clone()).collect();
        assert_eq!(sent, ["vol 41 [0.800, 0.400]"]);
        let shown = rt.world.borrow_mut().read(a, "volume").unwrap().to_number();
        assert!((shown - 0.6).abs() < 1e-6, "{shown}");
    }

    /// A volume above 100% set by another client (`wpctl set-volume 150%`, an app through
    /// pipewire-pulse, a restored state) is shown as it is and nothing is written back.
    #[test]
    fn a_volume_above_one_hundred_percent_from_another_client_is_shown_as_it_is() {
        let mut rt = Runtime::new(None);
        let root = rt.load_str(SINK_QML).unwrap();
        rt.settle();
        feed(&mut rt, acked(vec![0.5, 0.5], false, 0, 0));
        let sent = record(&rt);
        let a = audio_of(&rt, root);
        feed(&mut rt, acked(vec![1.5, 0.75], false, 0, 0));
        let shown = |rt: &Runtime| rt.world.borrow_mut().read(a, "volume").unwrap().to_number();
        assert_eq!(shown(&rt), 1.125);
        let chans = nums(rt.world.borrow_mut().read(a, "volumes")).unwrap();
        assert_eq!(chans, [1.5, 0.75]);
        feed(&mut rt, acked(vec![2.0, 2.0], false, 0, 0));
        assert_eq!(shown(&rt), 2.0);
        assert!(sent.borrow().is_empty(), "{:?}", sent.borrow());
    }

    /// `Audio.incrementVolume()` steps from the shown volume. Two quick steps send 0.6 and
    /// 0.7; the snapshot the backend publishes with the server's echo of 0.6 must not show 0.6
    /// again (the slider jumped back, and the next step sent 0.7 a second time).
    #[test]
    fn a_stale_echo_does_not_revert_a_newer_volume_write() {
        let mut rt = Runtime::new(None);
        let root = rt.load_str(SINK_QML).unwrap();
        rt.settle();
        feed(&mut rt, acked(vec![0.5, 0.5], false, 0, 0));
        let sent = record(&rt);
        let a = audio_of(&rt, root);
        let shown = |rt: &Runtime| rt.world.borrow_mut().read(a, "volume").unwrap().to_number();
        let step = |rt: &mut Runtime| {
            let v = shown(rt) + 0.1;
            rt.world.borrow_mut().write(a, "volume", Value::Num(v));
            rt.poll_native();
            rt.settle();
        };
        step(&mut rt);
        step(&mut rt);
        let ids: Vec<u64> = sent.borrow().iter().map(|s| s.1).collect();
        let &[w1, w2] = ids.as_slice() else {
            panic!("{:?}", sent.borrow())
        };
        assert!(w1 < w2);
        feed(&mut rt, acked(vec![0.6, 0.6], false, w1, 0));
        assert!(
            (shown(&rt) - 0.7).abs() < 1e-4,
            "reverted to {}",
            shown(&rt)
        );
        step(&mut rt);
        let w3 = sent.borrow().last().unwrap().1;
        // Acked up to the second write: still older than the third.
        feed(&mut rt, acked(vec![0.7, 0.7], false, w2, 0));
        assert!(
            (shown(&rt) - 0.8).abs() < 1e-4,
            "reverted to {}",
            shown(&rt)
        );
        feed(&mut rt, acked(vec![0.8, 0.8], false, w3, 0));
        let vols: Vec<String> = sent.borrow().iter().map(|s| s.0.clone()).collect();
        assert_eq!(
            vols,
            [
                "vol 41 [0.600, 0.600]",
                "vol 41 [0.700, 0.700]",
                "vol 41 [0.800, 0.800]"
            ],
            "a step was lost"
        );
        // Once the last write is acked, the server's value is shown again, also a later
        // change from elsewhere (another mixer).
        feed(&mut rt, acked(vec![0.3, 0.3], false, w3, 0));
        assert!(
            (shown(&rt) - 0.3).abs() < 1e-4,
            "external change lost: {}",
            shown(&rt)
        );
        // A mute ack is no volume ack.
        rt.world.borrow_mut().write(a, "volume", Value::Num(0.9));
        rt.poll_native();
        let w4 = sent.borrow().last().unwrap().1;
        feed(&mut rt, acked(vec![0.3, 0.3], false, w3, w4));
        assert!(
            (shown(&rt) - 0.9).abs() < 1e-4,
            "reverted to {}",
            shown(&rt)
        );
    }

    /// Two quick mute toggles: the echo of the first must not flip the switch back.
    #[test]
    fn a_stale_echo_does_not_revert_a_newer_mute_write() {
        let mut rt = Runtime::new(None);
        let root = rt.load_str(SINK_QML).unwrap();
        rt.settle();
        feed(&mut rt, acked(vec![0.5, 0.5], false, 0, 0));
        let sent = record(&rt);
        let a = audio_of(&rt, root);
        let muted = |rt: &Runtime| rt.world.borrow_mut().read(a, "muted").unwrap().truthy();
        for m in [true, false] {
            rt.world.borrow_mut().write(a, "muted", Value::Bool(m));
            rt.poll_native();
            rt.settle();
        }
        let ids: Vec<u64> = sent.borrow().iter().map(|s| s.1).collect();
        let &[w1, w2] = ids.as_slice() else {
            panic!("{:?}", sent.borrow())
        };
        feed(&mut rt, acked(vec![0.5, 0.5], true, 0, w1));
        assert!(
            !muted(&rt),
            "the echo of the first toggle unmuted the switch"
        );
        feed(&mut rt, acked(vec![0.5, 0.5], false, 0, w2));
        assert!(!muted(&rt));
        feed(&mut rt, acked(vec![0.5, 0.5], true, 0, w2));
        assert!(muted(&rt), "a later mute from elsewhere is not shown");
        assert_eq!(sent.borrow().len(), 2, "{:?}", sent.borrow());
    }

    /// `Pipewire.preferredDefaultAudioSink = x` while a snapshot naming the old configured
    /// default is in flight: that snapshot must not select the old sink again.
    #[test]
    fn a_stale_snapshot_does_not_revert_the_preferred_default() {
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(
                r#"import QtQuick
import Rustshell.Services.Pipewire
Item {
    readonly property string preferred: Pipewire.preferredDefaultAudioSink?.name ?? ""
    property int pick: -1
    onPickChanged: Pipewire.preferredDefaultAudioSink = Pipewire.nodes.values[pick]
}"#,
            )
            .unwrap();
        rt.settle();
        let snap = |configured: &str, ack: u64, nodes: &[u32]| Snap {
            connected: true,
            nodes: nodes
                .iter()
                .map(|&i| Arc::new(audio_node(i, vec![1.0])))
                .collect(),
            defaults: [
                String::new(),
                String::new(),
                configured.into(),
                String::new(),
            ],
            configured_ack: [ack, 0],
        };
        feed(&mut rt, snap("sink41", 0, &[41, 42]));
        assert_eq!(rt.get(root, "preferred").to_js_string(), "sink41");
        let sent = record(&rt);
        rt.set(root, "pick", Value::Num(1.0));
        rt.poll_native();
        rt.settle();
        let w1 = {
            let s = sent.borrow();
            assert_eq!(
                s.iter().map(|s| s.0.as_str()).collect::<Vec<_>>(),
                ["default default.configured.audio.sink sink42"]
            );
            s[0].1
        };
        feed(&mut rt, snap("sink41", 0, &[41, 42]));
        assert_eq!(rt.get(root, "preferred").to_js_string(), "sink42");
        feed(&mut rt, snap("sink42", w1, &[41, 42]));
        assert_eq!(rt.get(root, "preferred").to_js_string(), "sink42");
        feed(&mut rt, snap("sink41", w1, &[41, 42]));
        assert_eq!(rt.get(root, "preferred").to_js_string(), "sink41");
        assert_eq!(sent.borrow().len(), 1, "{:?}", sent.borrow());
    }

    /// A node that sent no channel map gets Quickshell's layout for its channel count: up to
    /// eight channels are named, any other count has no positions.
    #[test]
    fn default_layouts_cover_one_to_eight_channels() {
        assert_eq!(default_layout(2), [3, 4]);
        assert_eq!(default_layout(4), [3, 4, 12, 13]);
        assert_eq!(default_layout(6), [3, 4, 5, 6, 7, 8]);
        assert_eq!(default_layout(8), [3, 4, 5, 6, 12, 13, 7, 8]);
        for n in 1..=8 {
            assert_eq!(default_layout(n).len(), n);
        }
        assert!(default_layout(0).is_empty() && default_layout(9).is_empty());
    }

    const PREFER_QML: &str = r#"import QtQuick
import Rustshell.Services.Pipewire
Item {
    readonly property string preferred: Pipewire.preferredDefaultAudioSink?.name ?? ""
    property int pick: -2
    onPickChanged: Pipewire.preferredDefaultAudioSink = pick < 0 ? null : Pipewire.nodes.values[pick]
}"#;

    /// The sink `sink41` and the source `sink42` (a microphone), `configured` the configured
    /// default sink and `ack` the last write to it the server handled.
    fn sink_and_source(configured: &str, ack: u64) -> Snap {
        Snap {
            connected: true,
            nodes: vec![
                Arc::new(audio_node(41, vec![1.0])),
                Arc::new(NodeSnap {
                    ty: 9,
                    ..audio_node(42, vec![1.0])
                }),
            ],
            defaults: [
                String::new(),
                String::new(),
                configured.into(),
                String::new(),
            ],
            configured_ack: [ack, 0],
        }
    }

    /// `Pipewire.preferredDefaultAudioSink = null` clears the server's configured default, as
    /// PwDefaultTracker::changeConfiguredSink does with an empty name.
    #[test]
    fn null_clears_the_preferred_default() {
        let mut rt = Runtime::new(None);
        let root = rt.load_str(PREFER_QML).unwrap();
        rt.settle();
        feed(&mut rt, sink_and_source("sink41", 0));
        assert_eq!(rt.get(root, "preferred").to_js_string(), "sink41");
        let sent = record(&rt);
        rt.set(root, "pick", Value::Num(-1.0));
        rt.poll_native();
        rt.settle();
        let w = {
            let s = sent.borrow();
            assert_eq!(
                s.iter().map(|s| s.0.as_str()).collect::<Vec<_>>(),
                ["default default.configured.audio.sink (cleared)"]
            );
            s[0].1
        };
        assert_eq!(rt.get(root, "preferred").to_js_string(), "");
        // A snapshot from before the server handled the write does not bring the sink back.
        feed(&mut rt, sink_and_source("sink41", 0));
        assert_eq!(rt.get(root, "preferred").to_js_string(), "");
        feed(&mut rt, sink_and_source("", w));
        assert_eq!(rt.get(root, "preferred").to_js_string(), "");
        assert_eq!(sent.borrow().len(), 1, "{:?}", sent.borrow());
    }

    /// A node that is not an audio sink is refused as the preferred default sink, as
    /// PwDefaultTracker::changeConfiguredSink does: nothing is sent and the default stays.
    #[test]
    fn the_preferred_default_sink_refuses_a_source() {
        let mut rt = Runtime::new(None);
        let root = rt.load_str(PREFER_QML).unwrap();
        rt.settle();
        feed(&mut rt, sink_and_source("sink41", 0));
        assert_eq!(rt.get(root, "preferred").to_js_string(), "sink41");
        let sent = record(&rt);
        rt.set(root, "pick", Value::Num(1.0));
        rt.poll_native();
        rt.settle();
        assert!(sent.borrow().is_empty(), "{:?}", sent.borrow());
        assert_eq!(rt.get(root, "preferred").to_js_string(), "sink41");
    }

    /// A write whose node went away before its ack leaves the configured default to the
    /// server: it no longer names a node the shell has.
    #[test]
    fn a_pending_preferred_default_follows_the_server_once_its_node_is_gone() {
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(
                r#"import QtQuick
import Rustshell.Services.Pipewire
Item {
    readonly property string preferred: Pipewire.preferredDefaultAudioSink?.name ?? ""
    property int pick: -1
    onPickChanged: Pipewire.preferredDefaultAudioSink = Pipewire.nodes.values[pick]
}"#,
            )
            .unwrap();
        rt.settle();
        let snap = |configured: &str, nodes: &[u32]| Snap {
            connected: true,
            nodes: nodes
                .iter()
                .map(|&i| Arc::new(audio_node(i, vec![1.0])))
                .collect(),
            defaults: [
                String::new(),
                String::new(),
                configured.into(),
                String::new(),
            ],
            configured_ack: [0, 0],
        };
        feed(&mut rt, snap("sink41", &[41, 42]));
        let _sent = record(&rt);
        rt.set(root, "pick", Value::Num(1.0));
        rt.poll_native();
        rt.settle();
        assert_eq!(rt.get(root, "preferred").to_js_string(), "sink42");
        feed(&mut rt, snap("sink41", &[41]));
        assert_eq!(rt.get(root, "preferred").to_js_string(), "sink41");
    }

    fn feed_nodes(rt: &mut Runtime, nodes: Vec<NodeSnap>) {
        rt.poll_native();
        set_snap(
            &own(rt),
            Snap {
                nodes: nodes.into_iter().map(Arc::new).collect(),
                ..Default::default()
            },
        );
        rt.poll_native();
        rt.settle();
    }

    /// PipeWire hands a removed node's id to the next node: one removed and another created
    /// between two polls arrives under the same id. That is a new node (new `PwNode`, `ready`
    /// false until its own core sync), not the old one renamed: a reused ready object would
    /// show the new sink's initial volume as a change (volume OSD).
    #[test]
    fn a_reused_node_id_is_a_new_node() {
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(
                r#"import QtQuick
import Rustshell.Services.Pipewire
Item {
    readonly property var first: Pipewire.nodes.values[0] ?? null
    readonly property string desc: first ? first.name + ":" + first.ready : ""
    readonly property int count: Pipewire.nodes.values.length
    PwObjectTracker { objects: [first] }
}"#,
            )
            .unwrap();
        rt.settle();
        let node = |key: u64, name: &str, ready: bool, volumes: Vec<f32>| NodeSnap {
            key,
            name: name.into(),
            ready,
            ..audio_node(50, volumes)
        };
        feed_nodes(&mut rt, vec![node(7, "sink50", true, vec![1.0, 1.0])]);
        rt.poll_native();
        rt.settle();
        assert_eq!(rt.get(root, "desc").to_js_string(), "sink50:true");
        let before = rt.get(root, "first");
        let Value::Item(old) = before else {
            panic!("no node")
        };
        feed_nodes(&mut rt, vec![node(9, "other", false, vec![0.3, 0.3])]);
        let after = rt.get(root, "first");
        assert!(
            !before.strict_eq(&after),
            "the new node reuses the old PwNode object"
        );
        assert!(
            rt.world.borrow().dead.contains(&old),
            "old PwNode not destroyed"
        );
        assert_eq!(rt.get(root, "desc").to_js_string(), "other:false");
        assert_eq!(rt.get(root, "count").to_number(), 1.0);
        // The same key: the same node, updated in place.
        feed_nodes(&mut rt, vec![node(9, "other", true, vec![0.3, 0.3])]);
        rt.poll_native();
        rt.settle();
        assert!(rt.get(root, "first").strict_eq(&after));
        assert_eq!(rt.get(root, "desc").to_js_string(), "other:true");
    }

    /// A daemon restart between two polls: the client sees the old daemon's node, then (after
    /// the empty disconnected snapshot it never applies) the new daemon's node under the same
    /// id and `object.serial`, since a new daemon numbers its objects from the start again. It
    /// is still a new node: the old `PwNode` is destroyed and the new one not ready.
    #[test]
    fn a_node_of_a_restarted_daemon_is_new_despite_the_same_id_and_serial() {
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(
                r#"import QtQuick
import Rustshell.Services.Pipewire
Item {
    readonly property var first: Pipewire.nodes.values[0] ?? null
    readonly property bool ready: first?.ready ?? false
    PwObjectTracker { objects: [first] }
}"#,
            )
            .unwrap();
        rt.settle();
        let node = |serial: Option<u64>, ready: bool| NodeSnap {
            key: node_key(),
            serial,
            ready,
            ..audio_node(50, vec![0.5, 0.5])
        };
        for serial in [Some(7), None] {
            feed_nodes(&mut rt, vec![node(serial, true)]);
            rt.poll_native();
            rt.settle();
            assert!(rt.get(root, "ready").truthy());
            let Value::Item(old) = rt.get(root, "first") else {
                panic!("no node")
            };
            rt.poll_native();
            let sh = own(&rt);
            set_snap(&sh, Snap::default());
            set_snap(
                &sh,
                Snap {
                    nodes: vec![Arc::new(node(serial, false))],
                    connected: true,
                    ..Default::default()
                },
            );
            rt.poll_native();
            rt.settle();
            assert!(
                rt.world.borrow().dead.contains(&old),
                "the old daemon's PwNode is reused ({serial:?})"
            );
            assert!(!rt.get(root, "ready").truthy(), "{serial:?}");
        }
    }

    /// `Pipewire.ready`: the daemon's initial state is in, and false again once it is gone.
    #[test]
    fn pipewire_is_ready_only_while_connected() {
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(
                r#"import QtQuick
import Rustshell.Services.Pipewire
Item { readonly property bool ready: Pipewire.ready }"#,
            )
            .unwrap();
        rt.settle();
        assert!(!rt.get(root, "ready").truthy());
        for connected in [true, false, true] {
            rt.poll_native();
            set_snap(
                &own(&rt),
                Snap {
                    connected,
                    ..Default::default()
                },
            );
            rt.poll_native();
            rt.settle();
            assert_eq!(rt.get(root, "ready").truthy(), connected);
        }
    }

    /// quickshell's ObjectModel only changes on insertObject/removeAt: a volume change must
    /// not hand `Pipewire.nodes.values` a new array (every mixer delegate would rebuild).
    #[test]
    fn volume_changes_keep_the_node_model() {
        let mut rt = Runtime::new(None);
        rt.load_str(
            r#"import QtQuick
import Rustshell.Services.Pipewire
Item { readonly property int n: Pipewire.nodes.values.length }"#,
        )
        .unwrap();
        rt.settle();
        feed_nodes(&mut rt, vec![audio_node(41, vec![0.5, 0.5])]);
        let values = |rt: &Runtime| {
            let mut w = rt.world.borrow_mut();
            let root = w.singletons["Pipewire"];
            let Some(Value::Item(m)) = w.read(root, "nodes") else {
                panic!("no model")
            };
            w.read(m, "values").unwrap()
        };
        let before = values(&rt);
        feed_nodes(&mut rt, vec![audio_node(41, vec![0.8, 0.8])]);
        assert!(values(&rt).strict_eq(&before));
        feed_nodes(
            &mut rt,
            vec![
                audio_node(41, vec![0.8, 0.8]),
                audio_node(42, vec![1.0, 1.0]),
            ],
        );
        assert!(!values(&rt).strict_eq(&before));
    }

    /// Only QML writes reach PipeWire, and only writes on its own objects wake the service.
    #[test]
    fn qml_writes_are_forwarded_and_backend_updates_are_not() {
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(
                r#"import QtQuick
import Rustshell.Services.Pipewire
Item {
    property var sink: Pipewire.nodes.values[0] ?? null
    property real other: 0
    PwObjectTracker { objects: [sink] }
}"#,
            )
            .unwrap();
        rt.settle();
        feed_nodes(&mut rt, vec![audio_node(41, vec![0.5, 0.5])]);
        let sent = std::rc::Rc::new(std::cell::RefCell::new(vec![]));
        {
            let s = sent.clone();
            let mut w = rt.world.borrow_mut();
            w.pw.as_mut().unwrap().send = Box::new(move |c| s.borrow_mut().push(describe(&c)));
        }
        feed_nodes(&mut rt, vec![audio_node(41, vec![0.7, 0.7])]);
        assert!(sent.borrow().is_empty(), "{:?}", sent.borrow());

        rt.set(root, "other", Value::Num(1.0));
        assert!(!rt.world.borrow().has_watched(crate::svc::Svc::Pipewire));

        let audio = {
            let mut w = rt.world.borrow_mut();
            let Some(Value::Item(n)) = w.read(root, "sink") else {
                panic!("no sink")
            };
            let Some(Value::Item(a)) = w.read(n, "audio") else {
                panic!("no audio")
            };
            a
        };
        rt.world
            .borrow_mut()
            .write(audio, "muted", Value::Bool(true));
        assert!(rt.world.borrow().has_watched(crate::svc::Svc::Pipewire));
        rt.poll_native();
        assert_eq!(*sent.borrow(), ["mute 41 true"]);
        assert!(!rt.world.borrow().has_watched(crate::svc::Svc::Pipewire));
    }

    /// The backend publishes a node in steps (its global, then its info, its Props, its ready
    /// sync), which a busy runtime takes coalesced in any combination. A host that polls once
    /// per wakeup, like the shell's loop, still sees the node ready: the dispatch that writes
    /// `ready` after the snapshot carrying it is owed a wakeup of its own.
    #[test]
    fn a_node_turns_ready_with_one_poll_per_wakeup() {
        let qml = r#"import QtQuick
import Rustshell.Services.Pipewire
Item {
    readonly property var sink: Pipewire.nodes.values.find(n => n.name === "regecho") ?? null
    readonly property bool ready: sink?.ready ?? false
    PwObjectTracker { objects: [sink] }
}"#;
        let step = |stage: u8| {
            let mut n = NodeSnap {
                id: 40,
                key: 9,
                ty: 17,
                ..Default::default()
            };
            if stage >= 1 {
                n.name = "regecho".into();
                n.props = vec![("node.name".into(), "regecho".into())];
            }
            if stage >= 2 {
                n.volumes = vec![1.0, 1.0];
                n.channels = vec![3, 4];
            }
            n.ready = stage >= 3;
            Snap {
                nodes: vec![Arc::new(n)],
                connected: true,
                ..Default::default()
            }
        };
        // Which of the steps before ready the runtime sees on their own.
        for seen in 0u8..8 {
            let mut rt = Runtime::new(None);
            let wake = testenv::Wakeups::new();
            rt.world.borrow().waker.set_ping(wake.ping());
            let root = rt.load_str(qml).unwrap();
            rt.settle();
            rt.poll_native();
            rt.settle();
            let mut snaps = vec![Snap {
                connected: true,
                ..Default::default()
            }];
            snaps.extend((0..3).filter(|s| seen & (1 << s) != 0).map(step));
            snaps.push(step(3));
            for s in snaps {
                set_snap(&own(&rt), s);
                while wake.wait(None, Some(std::time::Duration::ZERO)).any() {
                    rt.poll_native();
                    rt.settle();
                }
            }
            assert!(
                rt.get(root, "ready").truthy(),
                "not ready after the steps {seen:03b} and no wakeup left"
            );
        }
    }
}
