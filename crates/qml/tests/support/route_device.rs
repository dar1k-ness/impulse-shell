//! A SPA device with one output Route, exported to the PipeWire daemon of the process
//! environment the way WirePlumber exports an ALSA card (`pw_core_export` of a
//! `SPA_TYPE_INTERFACE_Device`, served by module-client-device). Its Route behaves like ACP's:
//! `enum_params(Route)` lists it, `set_param(Route)` applies the given props, and every change
//! toggles `SPA_PARAM_INFO_SERIAL` of Route in an info event so that clients re-enumerate it.
//! A null-audio-sink node with `device.id` and `card.profile.device` pointing at it
//! ([`create_sink`]) then is a sink whose volume and mute live in that Route.
//!
//! The device has no Profile, so WirePlumber selects no profile or routes for it. Its route
//! restore would still set default volumes on the first Route change it sees, at a time the
//! test cannot wait for, so WirePlumber must run with [`write_wireplumber_conf`]: then only
//! the clients under test change the Route.

use pipewire as pw;
use pw::spa::pod::deserialize::PodDeserializer;
use pw::spa::pod::serialize::PodSerializer;
use pw::spa::pod::{Object, Pod, Property, Value as PV, ValueArray};
use pw::spa::sys as spa;
use pw::spa::utils::Id;
use std::ffi::{CString, c_int, c_void};
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex, mpsc};

/// The Route as the device holds it, in quickshell's visual volumes (cube roots of the
/// linear `channelVolumes`).
#[derive(Clone, Debug, PartialEq)]
pub struct RouteState {
    pub visual_volumes: Vec<f32>,
    pub muted: bool,
}

/// What the device thread reports to the test.
#[derive(Default)]
struct Log {
    /// The Route after each `set_param(Route)` from a client, in order.
    sets: Vec<RouteState>,
    /// `sets` already consumed by `wait_set`.
    seen: usize,
    /// The device thread has ended (its connection failed or it was told to quit).
    ended: bool,
}

type Shared = Arc<(Mutex<Log>, Condvar)>;

enum Msg {
    External(Vec<f32>),
    Quit,
}

/// The exported device; it lives on a thread of its own with its own main loop and
/// connection.
pub struct RouteDevice {
    id: u32,
    tx: pw::channel::Sender<Msg>,
    log: Shared,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl RouteDevice {
    /// Exports a device named `name` whose Route has `visual_volumes` (one per channel of
    /// FL, FR, ...) and is not muted; returns once the daemon has bound it to a global.
    pub fn start(name: &str, visual_volumes: &[f32]) -> RouteDevice {
        let log: Shared = Arc::default();
        let (tx, rx) = pw::channel::channel::<Msg>();
        let (id_tx, id_rx) = mpsc::channel::<u32>();
        let (name, volumes, l) = (name.to_string(), visual_volumes.to_vec(), log.clone());
        let thread = std::thread::Builder::new()
            .name("route-device".into())
            .spawn(move || {
                let end = Ended(l.clone());
                if let Err(e) = run(&name, &volumes, l, rx, id_tx) {
                    eprintln!("route device: {e}");
                }
                drop(end);
            })
            .unwrap();
        let id = id_rx
            .recv()
            .expect("the route device ended before it was bound");
        RouteDevice {
            id,
            tx,
            log,
            thread: Some(thread),
        }
    }

    /// The device's global id.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Waits for the next `set_param(Route)` from a client after which the Route matches
    /// `pred`, and returns that state; earlier sets that do not match are skipped.
    pub fn wait_set(&self, what: &str, pred: impl Fn(&RouteState) -> bool) -> RouteState {
        // Named in the output nextest shows when its slow-timeout ends a hang.
        eprintln!("waiting for the route device to get {what}");
        let (m, cv) = &*self.log;
        let mut log = m.lock().unwrap();
        loop {
            while log.seen < log.sets.len() {
                let s = log.sets[log.seen].clone();
                log.seen += 1;
                if pred(&s) {
                    return s;
                }
            }
            assert!(
                !log.ended,
                "the route device ended while waiting for {what}"
            );
            log = cv.wait(log).unwrap();
        }
    }

    /// Changes the Route's volumes the way the hardware or another client would: the device
    /// emits the param change, which does not count as a client's set for `wait_set`.
    pub fn set_external(&self, visual_volumes: &[f32]) {
        if self
            .tx
            .send(Msg::External(visual_volumes.to_vec()))
            .is_err()
        {
            panic!("the route device has ended");
        }
    }
}

impl Drop for RouteDevice {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Quit);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Creates a null-audio-sink node named `name` with channels FL, FR whose volume and mute are
/// those of `device`'s Route (`device.id`, `card.profile.device` as ACP's nodes have them).
pub fn create_sink(name: &str, device: u32) {
    let node = format!(
        "{{ factory.name=support.null-audio-sink media.class=Audio/Sink node.name={name} \
         audio.position=[FL,FR] device.id={device} card.profile.device={ROUTE_DEVICE} \
         object.linger=true }}"
    );
    let out = std::process::Command::new("pw-cli")
        .args(["create-node", "adapter", &node])
        .output()
        .unwrap();
    assert!(out.status.success(), "pw-cli: {out:?}");
}

/// Marks the log ended when the device thread exits, also by a panic.
struct Ended(Shared);

impl Drop for Ended {
    fn drop(&mut self) {
        let (m, cv) = &*self.0;
        if let Ok(mut l) = m.lock() {
            l.ended = true;
        }
        cv.notify_all();
    }
}

const ROUTE_INDEX: i32 = 0;
/// The card device (`card.profile.device`) the Route belongs to.
const ROUTE_DEVICE: i32 = 0;
const PROFILE: i32 = 1;
const CHANNELS: [u32; 2] = [spa::SPA_AUDIO_CHANNEL_FL, spa::SPA_AUDIO_CHANNEL_FR];
/// The index of Route in `Dev::params` (EnumRoute is 0).
const ROUTE: usize = 1;

/// The device, `spa_device` first so the interface pointer is the object.
#[repr(C)]
struct Dev {
    device: spa::spa_device,
    hooks: spa::spa_hook_list,
    params: [spa::spa_param_info; 2],
    /// Linear channel volumes.
    volumes: Vec<f32>,
    muted: bool,
    items: Vec<spa::spa_dict_item>,
    dict: spa::spa_dict,
    _strings: Vec<CString>,
    log: Shared,
}

static METHODS: spa::spa_device_methods = spa::spa_device_methods {
    version: spa::SPA_VERSION_DEVICE_METHODS,
    add_listener: Some(add_listener),
    sync: Some(sync),
    enum_params: Some(enum_params),
    set_param: Some(set_param),
};

fn param_info(id: u32, flags: u32) -> spa::spa_param_info {
    spa::spa_param_info {
        id,
        flags,
        user: 0,
        seq: 0,
        padding: [0; 4],
    }
}

impl Dev {
    /// A device at a stable address (C holds pointers into it): never moved out of the box.
    fn new(name: &str, visual: &[f32], log: Shared) -> Box<Dev> {
        let strings: Vec<CString> = [
            ("device.name", name),
            ("device.description", name),
            ("device.api", "test"),
        ]
        .iter()
        .flat_map(|(k, v)| [CString::new(*k).unwrap(), CString::new(*v).unwrap()])
        .collect();
        let items: Vec<spa::spa_dict_item> = strings
            .chunks(2)
            .map(|kv| spa::spa_dict_item {
                key: kv[0].as_ptr(),
                value: kv[1].as_ptr(),
            })
            .collect();
        let mut d = Box::new(Dev {
            device: spa::spa_device {
                iface: spa::spa_interface {
                    type_: c"Spa:Pointer:Interface:Device".as_ptr(),
                    version: spa::SPA_VERSION_DEVICE,
                    cb: spa::spa_callbacks {
                        funcs: std::ptr::from_ref(&METHODS).cast(),
                        data: std::ptr::null_mut(),
                    },
                },
            },
            hooks: spa::spa_hook_list {
                list: spa::spa_list {
                    next: std::ptr::null_mut(),
                    prev: std::ptr::null_mut(),
                },
            },
            params: [
                param_info(spa::SPA_PARAM_EnumRoute, spa::SPA_PARAM_INFO_READ),
                param_info(spa::SPA_PARAM_Route, spa::SPA_PARAM_INFO_READWRITE),
            ],
            volumes: visual.iter().map(|v| v * v * v).collect(),
            muted: false,
            dict: spa::spa_dict {
                flags: 0,
                n_items: items.len() as u32,
                items: items.as_ptr(),
            },
            items,
            _strings: strings,
            log,
        });
        let me: *mut Dev = &mut *d;
        d.device.iface.cb.data = me.cast();
        let head = &raw mut d.hooks.list;
        // spa_hook_list_init: an empty list points at itself.
        d.hooks.list.next = head;
        d.hooks.list.prev = head;
        d
    }

    fn state(&self) -> RouteState {
        RouteState {
            visual_volumes: self.volumes.iter().map(|v| v.cbrt()).collect(),
            muted: self.muted,
        }
    }

    fn route(&self, id: u32) -> Vec<u8> {
        let mut p = vec![
            Property::new(spa::SPA_PARAM_ROUTE_index, PV::Int(ROUTE_INDEX)),
            Property::new(
                spa::SPA_PARAM_ROUTE_direction,
                PV::Id(Id(spa::SPA_DIRECTION_OUTPUT)),
            ),
            Property::new(
                spa::SPA_PARAM_ROUTE_name,
                PV::String("analog-output".into()),
            ),
            Property::new(
                spa::SPA_PARAM_ROUTE_description,
                PV::String("Speakers".into()),
            ),
            Property::new(spa::SPA_PARAM_ROUTE_priority, PV::Int(100)),
            Property::new(
                spa::SPA_PARAM_ROUTE_available,
                PV::Id(Id(spa::SPA_PARAM_AVAILABILITY_yes)),
            ),
        ];
        if id == spa::SPA_PARAM_EnumRoute {
            p.push(Property::new(
                spa::SPA_PARAM_ROUTE_profiles,
                PV::ValueArray(ValueArray::Int(vec![PROFILE])),
            ));
            p.push(Property::new(
                spa::SPA_PARAM_ROUTE_devices,
                PV::ValueArray(ValueArray::Int(vec![ROUTE_DEVICE])),
            ));
        } else {
            p.push(Property::new(
                spa::SPA_PARAM_ROUTE_device,
                PV::Int(ROUTE_DEVICE),
            ));
            p.push(Property::new(
                spa::SPA_PARAM_ROUTE_profile,
                PV::Int(PROFILE),
            ));
            p.push(Property::new(
                spa::SPA_PARAM_ROUTE_props,
                PV::Object(Object {
                    type_: spa::SPA_TYPE_OBJECT_Props,
                    id: spa::SPA_PARAM_Route,
                    properties: vec![
                        Property::new(spa::SPA_PROP_mute, PV::Bool(self.muted)),
                        Property::new(
                            spa::SPA_PROP_channelVolumes,
                            PV::ValueArray(ValueArray::Float(self.volumes.clone())),
                        ),
                        Property::new(
                            spa::SPA_PROP_channelMap,
                            PV::ValueArray(ValueArray::Id(
                                CHANNELS.iter().map(|c| Id(*c)).collect(),
                            )),
                        ),
                    ],
                }),
            ));
            p.push(Property::new(spa::SPA_PARAM_ROUTE_save, PV::Bool(false)));
        }
        let o = PV::Object(Object {
            type_: spa::SPA_TYPE_OBJECT_ParamRoute,
            id,
            properties: p,
        });
        PodSerializer::serialize(std::io::Cursor::new(Vec::new()), &o)
            .unwrap()
            .0
            .into_inner()
    }

    /// Calls `f` with the events and data of each listener.
    fn each(&self, f: impl Fn(&spa::spa_device_events, *mut c_void)) {
        let head = &raw const self.hooks.list;
        let mut l = self.hooks.list.next;
        while !std::ptr::eq(l, head) {
            // `link` is the first field of `spa_hook`, so a list entry is its hook.
            let h = l.cast::<spa::spa_hook>();
            // SAFETY: `l` is a hook added by add_listener and not yet removed (spa_hook_remove
            // unlinks it); its successor is read before calling it, as spa_hook_list_call does.
            let next = unsafe { (*l).next };
            // SAFETY: as above.
            let cb = unsafe { (*h).cb };
            // SAFETY: add_listener stored a `spa_device_events` table as the hook's funcs.
            let ev = unsafe { &*cb.funcs.cast::<spa::spa_device_events>() };
            f(ev, cb.data);
            l = next;
        }
    }

    /// The device info; `full` for a new listener (ACP's emit_info with `full`).
    fn info(&self, full: bool) -> spa::spa_device_info {
        let mut mask = spa::SPA_DEVICE_CHANGE_MASK_PARAMS as u64;
        if full {
            mask |= (spa::SPA_DEVICE_CHANGE_MASK_FLAGS | spa::SPA_DEVICE_CHANGE_MASK_PROPS) as u64;
        }
        spa::spa_device_info {
            version: spa::SPA_VERSION_DEVICE_INFO,
            change_mask: mask,
            flags: 0,
            props: &raw const self.dict,
            params: self.params.as_ptr().cast_mut(),
            n_params: self.params.len() as u32,
        }
    }

    /// The Route changed: toggles its `SPA_PARAM_INFO_SERIAL` and emits the info, as ACP does.
    fn route_changed(&mut self) {
        self.params[ROUTE].flags ^= spa::SPA_PARAM_INFO_SERIAL;
        self.params[ROUTE].user += 1;
        let info = self.info(false);
        self.each(|ev, data| {
            if let Some(f) = ev.info {
                // SAFETY: a listener's info callback with its own data and a valid info.
                unsafe { f(data, &info) };
            }
        });
    }

    fn result(&self, seq: c_int, ty: u32, result: *const c_void) {
        self.each(|ev, data| {
            if let Some(f) = ev.result {
                // SAFETY: a listener's result callback with its own data; `result` matches `ty`.
                unsafe { f(data, seq, 0, ty, result) };
            }
        });
    }

    fn set_route(&mut self, props: &Object) {
        for p in &props.properties {
            match (p.key, &p.value) {
                // ACP: fewer volumes than channels repeat over the channels.
                (spa::SPA_PROP_channelVolumes, PV::ValueArray(ValueArray::Float(v)))
                    if !v.is_empty() =>
                {
                    self.volumes = (0..CHANNELS.len()).map(|i| v[i % v.len()]).collect()
                }
                (spa::SPA_PROP_mute, PV::Bool(m)) => self.muted = *m,
                _ => {}
            }
        }
        self.route_changed();
    }
}

/// Writes a WirePlumber drop-in under `config_home` (the `XDG_CONFIG_HOME` WirePlumber runs
/// with) that turns its route restore (`device.restore-routes`) off.
pub fn write_wireplumber_conf(config_home: &Path) {
    let d = config_home.join("wireplumber/wireplumber.conf.d");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(
        d.join("route-device.conf"),
        "wireplumber.settings = { device.restore-routes = false }\n",
    )
    .unwrap();
}

/// The `Dev` behind a method's `object`.
///
/// # Safety
/// `object` is the `cb.data` of a `Dev`'s interface, called on the device thread.
unsafe fn dev<'a>(object: *mut c_void) -> &'a mut Dev {
    // SAFETY: per the contract, `object` points at a live `Dev` nobody else borrows now.
    unsafe { &mut *object.cast::<Dev>() }
}

unsafe extern "C" fn add_listener(
    object: *mut c_void,
    listener: *mut spa::spa_hook,
    events: *const spa::spa_device_events,
    data: *mut c_void,
) -> c_int {
    // SAFETY: called through this device's interface.
    let d = unsafe { dev(object) };
    // ACP: the new listener alone gets the full info (the device has no objects), then it
    // joins the others.
    let info = d.info(true);
    // SAFETY: the caller passes a valid events table for the listener's lifetime.
    if let Some(f) = unsafe { (*events).info } {
        // SAFETY: the listener's info callback with its own data.
        unsafe { f(data, &info) };
    }
    // SAFETY: `listener` is the caller's hook, valid until it removes it; `hooks` was
    // initialized and does not move.
    unsafe { spa::spa_hook_list_append(&raw mut d.hooks, listener, events.cast(), data) };
    0
}

unsafe extern "C" fn sync(object: *mut c_void, seq: c_int) -> c_int {
    // SAFETY: called through this device's interface.
    let d = unsafe { dev(object) };
    d.result(seq, 0, std::ptr::null());
    0
}

unsafe extern "C" fn enum_params(
    object: *mut c_void,
    seq: c_int,
    id: u32,
    start: u32,
    num: u32,
    filter: *const spa::spa_pod,
) -> c_int {
    // SAFETY: called through this device's interface.
    let d = unsafe { dev(object) };
    if id != spa::SPA_PARAM_EnumRoute && id != spa::SPA_PARAM_Route {
        return -libc::ENOENT;
    }
    if start > 0 || num == 0 {
        return 0;
    }
    let pod = d.route(id);
    let mut buf = [0u8; 4096];
    let mut param: *mut spa::spa_pod = std::ptr::null_mut();
    // SAFETY: an all-zero builder is valid input for spa_pod_builder_init.
    let mut b: spa::spa_pod_builder = unsafe { std::mem::zeroed() };
    // SAFETY: `buf` outlives the builder's use below.
    unsafe { spa::spa_pod_builder_init(&mut b, buf.as_mut_ptr().cast(), buf.len() as u32) };
    // SAFETY: `pod` is a complete serialized pod and `filter` is null or a valid pod.
    let res = unsafe { spa::spa_pod_filter(&mut b, &mut param, pod.as_ptr().cast(), filter) };
    if res < 0 {
        return 0;
    }
    let r = spa::spa_result_device_params {
        id,
        index: 0,
        next: 1,
        param,
    };
    d.result(
        seq,
        spa::SPA_RESULT_TYPE_DEVICE_PARAMS,
        std::ptr::from_ref(&r).cast(),
    );
    0
}

unsafe extern "C" fn set_param(
    object: *mut c_void,
    id: u32,
    _flags: u32,
    param: *const spa::spa_pod,
) -> c_int {
    // SAFETY: called through this device's interface.
    let d = unsafe { dev(object) };
    if id != spa::SPA_PARAM_Route {
        return -libc::ENOENT;
    }
    if param.is_null() {
        return -libc::EINVAL;
    }
    // SAFETY: the caller passes a valid pod for the duration of the call.
    let pod = unsafe { Pod::from_raw(param) };
    let Ok((_, PV::Object(o))) = PodDeserializer::deserialize_any_from(pod.as_bytes()) else {
        return -libc::EINVAL;
    };
    let (mut index, mut device, mut props) = (-1, -1, None);
    for p in o.properties {
        match (p.key, p.value) {
            (spa::SPA_PARAM_ROUTE_index, PV::Int(i)) => index = i,
            (spa::SPA_PARAM_ROUTE_device, PV::Int(i)) => device = i,
            (spa::SPA_PARAM_ROUTE_props, PV::Object(po)) => props = Some(po),
            _ => {}
        }
    }
    if index != ROUTE_INDEX || device != ROUTE_DEVICE {
        return -libc::EINVAL;
    }
    if let Some(props) = props {
        d.set_route(&props);
    }
    let (m, cv) = &*d.log;
    m.lock().unwrap().sets.push(d.state());
    cv.notify_all();
    0
}

/// `pw_proxy_events::bound`: the daemon registered the device as global `id`.
unsafe extern "C" fn bound(data: *mut c_void, id: u32) {
    // SAFETY: `data` is the sender `run` registered with this listener and keeps alive.
    let tx = unsafe { &*data.cast::<mpsc::Sender<u32>>() };
    let _ = tx.send(id);
}

fn run(
    name: &str,
    visual: &[f32],
    log: Shared,
    rx: pw::channel::Receiver<Msg>,
    id_tx: mpsc::Sender<u32>,
) -> Result<(), pw::Error> {
    pw::init();
    let ml = pw::main_loop::MainLoopRc::new(None)?;
    let ctx = pw::context::ContextRc::new(&ml, None)?;
    let core = ctx.connect_rc(None)?;
    let _core_l = core
        .add_listener_local()
        .error({
            let ml = ml.downgrade();
            move |id, _seq, res, msg| {
                eprintln!("route device: core error {id} {res} {msg}");
                if id == pw::core::PW_ID_CORE
                    && let Some(ml) = ml.upgrade()
                {
                    ml.quit();
                }
            }
        })
        .register();
    let dev = Box::into_raw(Dev::new(name, visual, log));
    let id_tx = Box::new(id_tx);
    // SAFETY: all-zero is a valid `pw_proxy_events` (every callback `None`) and `spa_hook`.
    let mut events: Box<pw::sys::pw_proxy_events> = Box::new(unsafe { std::mem::zeroed() });
    events.version = pw::sys::PW_VERSION_PROXY_EVENTS;
    events.bound = Some(bound);
    // SAFETY: as above.
    let mut hook: Box<spa::spa_hook> = Box::new(unsafe { std::mem::zeroed() });
    // SAFETY: `dev` is a live box.
    let dict = unsafe { &raw const (*dev).dict };
    // SAFETY: `dev` starts with its `spa_device` and stays valid until after the proxy is
    // destroyed; the dict is copied into the request.
    let proxy = unsafe {
        pw::sys::pw_core_export(
            core.as_raw_ptr(),
            c"Spa:Pointer:Interface:Device".as_ptr(),
            dict,
            dev.cast(),
            0,
        )
    };
    if proxy.is_null() {
        // SAFETY: `dev` came from Box::into_raw and nothing else refers to it.
        drop(unsafe { Box::from_raw(dev) });
        return Err(pw::Error::CreationFailed);
    }
    // SAFETY: `hook`, `events` and `id_tx` are boxed and outlive the listener, which is
    // removed before they are dropped.
    unsafe {
        pw::sys::pw_proxy_add_listener(
            proxy,
            &mut *hook,
            &*events,
            std::ptr::from_ref(&*id_tx).cast_mut().cast(),
        )
    };
    let _rx = rx.attach(ml.loop_(), {
        let ml = ml.downgrade();
        move |m| match m {
            Msg::External(v) => {
                // SAFETY: the device is only used on this thread and not borrowed here.
                let d = unsafe { &mut *dev };
                d.volumes = v.iter().map(|x| x * x * x).collect();
                d.route_changed();
            }
            Msg::Quit => {
                if let Some(ml) = ml.upgrade() {
                    ml.quit();
                }
            }
        }
    });
    ml.run();
    // SAFETY: `hook` is linked into the live proxy's listener list.
    unsafe { spa::spa_hook_remove(&mut *hook) };
    // SAFETY: the proxy is ours; destroying it removes module-client-device's listener from
    // the device, which is still alive.
    unsafe { pw::sys::pw_proxy_destroy(proxy) };
    // SAFETY: `dev` came from Box::into_raw and the proxy no longer refers to it.
    drop(unsafe { Box::from_raw(dev) });
    Ok(())
}
