//! `HyprlandData` against a fake Hyprland whose state changes between events: whatever burst of
//! events arrives (also while a query is still out), the data ends equal to a fresh read, each
//! event refetches only the queries it can affect, and a burst costs one round of them.

mod hypr_ipc;

use qml::runtime::Runtime;
use qml::value::Value;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard, OnceLock};
use testenv::Wakeups;

fn ri() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

/// Compositor state version: every reply shows the version current when the request came in.
static GEN: AtomicU64 = AtomicU64::new(1);
/// Whether the fake holds its replies, and how many it holds right now.
static HOLD: (Mutex<(bool, usize)>, Condvar) = (Mutex::new((false, 0)), Condvar::new());
/// The test loop's wakeup: the runtime's waker ping, and the fake's when a request or an
/// event connection comes in.
static WAKE: OnceLock<Wakeups> = OnceLock::new();
static PING: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();
/// Event lines written so far, and the root item counting the `rawEvent`s it got.
static SENT: AtomicUsize = AtomicUsize::new(0);
static ROOT: AtomicUsize = AtomicUsize::new(0);
static REQS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static EVENTS: Mutex<Vec<UnixStream>> = Mutex::new(Vec::new());

fn reply(cmd: &str, g: u64) -> String {
    match cmd.strip_prefix("j/").unwrap_or(cmd) {
        "clients" => format!(
            r#"[{{"address":"0x{g}","class":"foot","title":"t{g}","workspace":{{"id":1,"name":"1"}},"at":[0,0],"size":[10,10]}}]"#
        ),
        "monitors" => format!(
            r#"[{{"id":0,"name":"DP-1","width":1920,"height":1080,"activeWorkspace":{{"id":1,"name":"1"}},"focused":true,"reserved":[0,{g},0,0]}}]"#
        ),
        "layers" => {
            format!(r#"{{"DP-1":{{"levels":{{"2":[{{"address":"0xb","namespace":"ns{g}"}}]}}}}}}"#)
        }
        "workspaces" => format!(r#"[{{"id":1,"name":"1","monitor":"DP-1","windows":{g}}}]"#),
        "activeworkspace" => format!(r#"{{"id":1,"name":"1","monitor":"DP-1","windows":{g}}}"#),
        "activewindow" => "{}".into(),
        _ => "ok".into(),
    }
}

/// Whether this is `test`'s own process (`testenv::run_in_own_process`), whose
/// `XDG_RUNTIME_DIR` and `HYPRLAND_INSTANCE_SIGNATURE` name a fresh directory for [`fake`]
/// and whose XDG config, state and cache homes are under it. Elsewhere it runs `test` there
/// and returns false.
fn own_process(test: &str) -> bool {
    if testenv::in_own_process(test) {
        return true;
    }
    let rt = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(rt.path().join("hypr/EVTEST")).unwrap();
    testenv::run_in_own_process(
        test,
        [
            ("XDG_RUNTIME_DIR", rt.path().as_os_str()),
            (
                "HYPRLAND_INSTANCE_SIGNATURE",
                std::ffi::OsStr::new("EVTEST"),
            ),
            ("XDG_CONFIG_HOME", rt.path().join("config").as_os_str()),
            ("XDG_STATE_HOME", rt.path().join("state").as_os_str()),
            ("XDG_CACHE_HOME", rt.path().join("cache").as_os_str()),
        ],
    );
    false
}

/// One fake instance per process, in the directory [`own_process`] made; calls take it in turn.
fn fake() -> MutexGuard<'static, ()> {
    static F: OnceLock<Mutex<()>> = OnceLock::new();
    let m = F.get_or_init(|| {
        let dir = Path::new(&std::env::var_os("XDG_RUNTIME_DIR").unwrap()).join("hypr/EVTEST");
        let req = UnixListener::bind(dir.join(".socket.sock")).unwrap();
        // Hyprland writes its lock once its sockets listen.
        std::fs::write(dir.join("hyprland.lock"), "1\nwayland-1\n").unwrap();
        std::thread::spawn(move || {
            for mut s in req.incoming().flatten() {
                std::thread::spawn(move || {
                    let mut buf = [0u8; 4096];
                    let n = s.read(&mut buf).unwrap_or(0);
                    let cmd = String::from_utf8_lossy(&buf[..n]).to_string();
                    let g = GEN.load(Ordering::SeqCst);
                    REQS.lock().unwrap().push(cmd.clone());
                    let (hold, cv) = &HOLD;
                    let mut h = hold.lock().unwrap();
                    h.1 += 1;
                    ping();
                    h = cv.wait_while(h, |h| h.0).unwrap();
                    h.1 -= 1;
                    drop(h);
                    let _ = s.write_all(hypr_ipc::answer(&cmd, |c| reply(c, g)).as_bytes());
                });
            }
        });
        let ev = UnixListener::bind(dir.join(".socket2.sock")).unwrap();
        std::thread::spawn(move || {
            for c in ev.incoming().flatten() {
                EVENTS.lock().unwrap().push(c);
                ping();
            }
        });
        Mutex::new(())
    });
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn ping() {
    if let Some(p) = PING.get() {
        p();
    }
}

/// Holds every reply from now on (`true`), or lets the held ones and all later go.
fn hold(on: bool) {
    HOLD.0.lock().unwrap().0 = on;
    HOLD.1.notify_all();
}

/// `lines` in one write on every event connection (one burst).
fn events(lines: &[String]) {
    SENT.fetch_add(lines.len(), Ordering::SeqCst);
    let mut all = lines.join("\n");
    all.push('\n');
    for c in EVENTS.lock().unwrap().iter_mut() {
        let _ = c.write_all(all.as_bytes());
    }
}

/// Runs loop turns, sleeping on [`WAKE`] between them, until `done`.
fn until(rt: &mut Runtime, what: &str, mut done: impl FnMut(&mut Runtime) -> bool) {
    // Named in the output nextest shows when its slow-timeout ends a hang.
    eprintln!("waiting for {what}");
    let wake = WAKE.get().unwrap();
    let ep = rt.process_fd();
    loop {
        // What a poll writes counts only for the next one.
        loop {
            let rev = rt.revision();
            rt.poll_native();
            rt.settle();
            if rt.revision() == rev {
                break;
            }
        }
        if done(rt) {
            return;
        }
        if wake.wait(ep, None).fd {
            rt.process_fd_ready();
        }
    }
}

/// Every event line written so far reached `HyprlandData` as a `rawEvent`.
fn delivered(rt: &mut Runtime) -> bool {
    rt.get(ROOT.load(Ordering::SeqCst), "events").to_number() as usize
        == SENT.load(Ordering::SeqCst)
}

fn idle(rt: &mut Runtime) -> bool {
    qml::hyprctl::in_flight(&rt.world.borrow()) == 0 && !rt.hyprland_refreshing()
}

/// Until every event sent arrived and no query is out or queued.
fn settle(rt: &mut Runtime) {
    until(rt, "the events and their queries", |rt| {
        delivered(rt) && idle(rt)
    });
}

fn load() -> (Runtime, usize) {
    let mut rt = Runtime::new(Some(ri()));
    let wake = WAKE.get_or_init(Wakeups::new);
    rt.world.borrow().waker.set_ping(wake.ping());
    let _ = PING.set(Box::new(wake.ping()));
    rt.load_singletons_where(&[ri().join("services")], &|n| n == "HyprlandData");
    let r = rt
        .load_str(
            "import QtQuick\nimport Rustshell.Hyprland\nimport ri.services\n\
             Item { property var s: HyprlandData\n property int events: 0\n\
             Connections { target: Hyprland; function onRawEvent(e) { events++ } } }",
        )
        .unwrap();
    ROOT.store(r, Ordering::SeqCst);
    let Value::Item(s) = rt.get(r, "s") else {
        panic!("no s: {:?}", rt.errors())
    };
    // The event connection is up before the first event, and the startup queries are done.
    until(
        &mut rt,
        "the event connection and the startup queries",
        |rt| {
            !EVENTS.lock().unwrap().is_empty()
                && idle(rt)
                && js(rt, s, &["windowList", "0", "address"]) == "0x1"
        },
    );
    (rt, s)
}

fn js(rt: &mut Runtime, s: usize, path: &[&str]) -> String {
    let mut v = rt.get(s, path[0]);
    for k in &path[1..] {
        v = match &v {
            Value::Obj(o) => {
                let o = o.borrow();
                match k.parse::<usize>() {
                    Ok(i) if o.array.is_some() => o
                        .array
                        .as_ref()
                        .unwrap()
                        .get(i)
                        .cloned()
                        .unwrap_or_default(),
                    _ => o.props.get(*k).cloned().unwrap_or_default(),
                }
            }
            _ => Value::Undefined,
        };
    }
    v.to_js_string()
}

/// (clients, monitors, layers, workspaces, activeworkspace) HyprlandData sent; the native
/// `Hyprland` singleton refreshes with one `[[BATCH]]` request, which none of these match.
fn data_reqs() -> [usize; 5] {
    let r = REQS.lock().unwrap();
    let n = |w: &str| r.iter().filter(|x| *x == w).count();
    [
        n("j/clients"),
        n("j/monitors"),
        n("j/layers"),
        n("j/workspaces"),
        n("j/activeworkspace"),
    ]
}

/// The last event of a burst lands while the previous event's queries are still out: its
/// queries must still run once those finish, so the data matches the compositor afterwards.
#[test]
fn event_during_inflight_query_is_not_lost() {
    if !own_process(concat!(
        module_path!(),
        "::event_during_inflight_query_is_not_lost"
    )) {
        return;
    }
    let _f = fake();
    let (mut rt, s) = load();
    hold(true);
    GEN.fetch_add(1, Ordering::SeqCst);
    let before = REQS.lock().unwrap().len();
    events(&["openwindow>>a,1,foot,foot".into()]);
    // One of HyprlandData's own queries (not the `Hyprland` singleton's batch) is held.
    until(&mut rt, "the first event's queries to be out", |rt| {
        delivered(rt)
            && REQS.lock().unwrap()[before..]
                .iter()
                .any(|r| r.starts_with("j/"))
    });
    assert!(
        qml::hyprctl::in_flight(&rt.world.borrow()) > 0,
        "queries out"
    );
    let g = GEN.fetch_add(1, Ordering::SeqCst) + 1;
    events(&["closewindow>>a".into()]);
    until(&mut rt, "the second event", delivered);
    assert!(
        qml::hyprctl::in_flight(&rt.world.borrow()) > 0,
        "the first event's queries still out"
    );
    hold(false);
    settle(&mut rt);
    assert_eq!(
        js(&mut rt, s, &["windowList", "0", "address"]),
        format!("0x{g}")
    );
    assert_eq!(js(&mut rt, s, &["addresses", "0"]), format!("0x{g}"));
    assert_eq!(
        js(&mut rt, s, &["workspaces", "0", "windows"]),
        g.to_string()
    );
    assert_eq!(
        js(&mut rt, s, &["activeWorkspace", "windows"]),
        g.to_string()
    );
}

/// Layer surfaces opening/closing change `layers` (and the monitors' reserved area): the
/// original skipped those events and kept stale layers.
#[test]
fn layer_events_refresh_layers() {
    if !own_process(concat!(module_path!(), "::layer_events_refresh_layers")) {
        return;
    }
    let _f = fake();
    let (mut rt, s) = load();
    let g = GEN.fetch_add(1, Ordering::SeqCst) + 1;
    events(&["openlayer>>rustshell:bar".into()]);
    settle(&mut rt);
    assert_eq!(
        js(
            &mut rt,
            s,
            &["layers", "DP-1", "levels", "2", "0", "namespace"]
        ),
        format!("ns{g}")
    );
    assert_eq!(
        js(&mut rt, s, &["monitors", "0", "reserved", "1"]),
        g.to_string()
    );
}

/// A burst of focus changes refetches clients and workspaces about once, and never the
/// monitors or layers it cannot change.
#[test]
fn event_burst_coalesces_to_affected_queries() {
    if !own_process(concat!(
        module_path!(),
        "::event_burst_coalesces_to_affected_queries"
    )) {
        return;
    }
    let _f = fake();
    let (mut rt, s) = load();
    REQS.lock().unwrap().clear();
    let g = GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let burst: Vec<String> = (0..40)
        .flat_map(|i| {
            [
                format!("activewindow>>foot,t{i}"),
                format!("activewindowv2>>{i:x}"),
            ]
        })
        .collect();
    // The whole burst reaches the shell while the first round of queries it starts is out
    // (its replies held), as when the compositor writes it faster than one round trip.
    hold(true);
    events(&burst);
    until(&mut rt, "the burst", delivered);
    hold(false);
    settle(&mut rt);
    let [clients, monitors, layers, workspaces, active] = data_reqs();
    assert_eq!((monitors, layers), (0, 0), "unaffected queries ran");
    for (what, n) in [
        ("clients", clients),
        ("workspaces", workspaces),
        ("activeworkspace", active),
    ] {
        assert!((1..=2).contains(&n), "{what}: {n} requests for one burst");
    }
    assert_eq!(
        js(&mut rt, s, &["windowList", "0", "title"]),
        format!("t{g}")
    );
    assert_eq!(
        js(&mut rt, s, &["activeWorkspace", "windows"]),
        g.to_string()
    );
}
