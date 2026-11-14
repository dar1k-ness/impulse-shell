//! Brightness.qml's DDC branch: `initialize` (`DisplayBrightness.readDdc`, what
//! `ddcutil -b BUS getvcp 10 --brief` was) and `syncBrightness` (`DisplayBrightness.setDdc`,
//! what `ddcutil -b BUS setvcp 10 V` was). The type talks DDC/CI over ddc-i2c, here through
//! `brightness::Roots::ddc` answering from a VCP file (a DDC/CI monitor cannot be faked on
//! i2c). Checked: the monitor's `rawMaxBrightness`/`brightness`/`ready`, the VCP written and
//! every value set, as ddcutil's arguments were. The backlight branch is
//! `brightness.rs::matches_real_brightnessctl`.

use qml::brightness::{self, Roots};
use qml::runtime::Runtime;
use qml::value::Value;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use testenv::Wakeups;

static VCP: Mutex<Option<PathBuf>> = Mutex::new(None);

fn ri_src() -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml/services/Brightness.qml"),
    )
    .unwrap()
}

/// Brightness.qml reduced to the DDC members, verbatim from qml.
fn reduced_brightness_qml() -> String {
    let src = ri_src();
    let block = |start: &str| {
        let i = src.find(start).unwrap_or_else(|| panic!("{start}"));
        let mut depth = 0;
        for (k, c) in src[i..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return src[i..=i + k].to_string();
                    }
                }
                _ => {}
            }
        }
        panic!("{start}")
    };
    format!(
        "import QtQuick\nimport Quickshell\nimport Impulse\nItem {{\n id: root\n signal brightnessChanged()\n property var ddcMonitors: []\n\
         readonly property list<BrightnessMonitor> monitors: Rustshell.screens.map(screen => monitorComp.createObject(root, {{ screen: screen }}))\n\
         property var m0: monitors[0]\n\
         {}\n{}\n{}\n{}\n\
         component BrightnessMonitor: QtObject {{\n id: monitor\n required property ShellScreen screen\n property bool isDdc\n property string busNum\n\
         property int rawMaxBrightness: 100\n property real brightness\n property real brightnessMultiplier: 1.0\n property real multipliedBrightness: brightness\n property bool ready: false\n property var externalLevel: null\n\
         {}\n{}\n{}\n property var setTimer: Timer {{ id: setTimer; interval: monitor.isDdc ? 300 : 0; onTriggered: syncBrightness() }}\n\
         onMultipliedBrightnessChanged: setTimer.restart()\n {}\n }}\n Component {{ id: monitorComp; BrightnessMonitor {{}} }}\n}}\n",
        block("onMonitorsChanged:"),
        block("function initializeMonitor("),
        block("function ddcDetectFinished("),
        block("Connections {\n        target: DisplayBrightness"),
        block("function initialize()"),
        block("readonly property Connections levels:"),
        block("function syncBrightness()"),
        block("function setBrightness("),
    )
}

/// The runtime on a virtual clock (`Runtime::tick`) instead of wall time. Runs what is due,
/// blocking on the runtime's wakeups while work it started is unanswered (`busy`); a hang is
/// nextest's slow-timeout to end.
struct Clock {
    wake: Wakeups,
    ep: Option<i32>,
    now: f64,
}

impl Clock {
    fn new(rt: &mut Runtime) -> Clock {
        let wake = Wakeups::new();
        rt.world.borrow().waker.set_ping(wake.ping());
        let ep = rt.process_fd();
        Clock { wake, ep, now: 0.0 }
    }

    /// Runs everything due at `now`, until no work is unanswered and no wakeup is pending.
    fn drain(&mut self, rt: &mut Runtime) {
        loop {
            workpool::wait_idle();
            rt.poll_native();
            rt.tick(self.now);
            rt.poll_native();
            rt.settle();
            let busy = busy(&rt.world.borrow());
            let woke = self.wake.wait(self.ep, (!busy).then_some(Duration::ZERO));
            if woke.fd {
                rt.process_fd_ready();
            }
            if !busy && !woke.any() && rt.next_deadline(self.now) > 0.0 {
                return;
            }
        }
    }

    /// Moves the clock `ms` on, timer by timer, draining at each.
    fn advance(&mut self, rt: &mut Runtime, ms: f64) {
        let end = self.now + ms;
        loop {
            self.drain(rt);
            let d = rt.next_deadline(self.now);
            if self.now + d > end {
                self.now = end;
                self.drain(rt);
                return;
            }
            self.now += d;
        }
    }
}

/// A process or a brightness job is unanswered.
fn busy(w: &qml::runtime::World) -> bool {
    !w.procs.is_empty() || brightness::busy(w)
}

fn read_vcp(f: &Path) -> (u32, u32) {
    let s = std::fs::read_to_string(f).unwrap();
    let mut it = s.split_whitespace().map(|x| x.parse().unwrap());
    (it.next().unwrap(), it.next().unwrap())
}

/// The monitor on bus 5 behind `VCP`; logs its writes as ddcutil's argv tail.
fn hook(bus: u32, set: Option<u16>) -> Option<(u32, u32)> {
    let f = VCP.lock().unwrap().clone()?;
    if bus != 5 {
        return None;
    }
    let (cur, max) = read_vcp(&f);
    if let Some(v) = set {
        std::fs::write(&f, format!("{v} {max}\n")).unwrap();
        let mut log = std::fs::read_to_string(f.with_extension("set")).unwrap_or_default();
        log.push_str(&format!("-b 5 setvcp 10 {v}\n"));
        std::fs::write(f.with_extension("set"), log).unwrap();
        return Some((v as u32, max));
    }
    Some((cur, max))
}

struct Run {
    raw_max: f64,
    brightness: f64,
    ready: bool,
    vcp: (u32, u32),
    sets: String,
}

/// One monitor on bus 5 at `cur`/`max`, in the directory `d`: its state once read, then
/// `setBrightness(level)`.
fn run(d: &Path, cur: u32, max: u32, level: f64) -> Run {
    let sys = d.join("sys");
    let conn = sys.join("class/drm/card1-DP-1");
    std::fs::create_dir_all(conn.join("i2c-5")).unwrap();
    std::fs::write(conn.join("status"), "connected\n").unwrap();
    let vcp = d.join("vcp");
    std::fs::write(&vcp, format!("{cur} {max}\n")).unwrap();
    *VCP.lock().unwrap() = Some(vcp.clone());
    brightness::set_roots(Roots {
        sys: sys.clone(),
        dev: sys.join("dev"),
        logind: None,
        ddc: Some(hook),
    });
    let file = d.join("services/Brightness.qml");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, reduced_brightness_qml()).unwrap();
    let mut rt = Runtime::new(None);
    let mut clock = Clock::new(&mut rt);
    rt.set_screens(&[("DP-1".into(), 0, 0, 100, 100)]);
    let root = rt.load_file(&file).unwrap();
    // 4.8 s of the runtime's clock: past setTimer's 300 ms, the jobs answered.
    let mut step = |rt: &mut Runtime| clock.advance(rt, 4800.0);
    step(&mut rt);
    let Value::Item(m) = rt.get(root, "m0") else {
        panic!("no monitor")
    };
    let r = (
        rt.get(m, "rawMaxBrightness").to_number(),
        rt.get(m, "brightness").to_number(),
        rt.get(m, "ready").truthy(),
    );
    let _ = rt.call_method(m, "setBrightness", vec![Value::Num(level)]);
    step(&mut rt);
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    assert!(rt.world.borrow().procs.is_empty(), "a process was started");
    drop(rt);
    *VCP.lock().unwrap() = None;
    let read = |f: &str| std::fs::read_to_string(d.join(f)).unwrap_or_default();
    Run {
        raw_max: r.0,
        brightness: r.1,
        ready: r.2,
        vcp: read_vcp(&vcp),
        sets: read("vcp.set"),
    }
}

#[test]
fn initialize_and_sync_brightness_over_ddc() {
    let d = tempfile::tempdir().unwrap();
    let d = d.path();
    // (current, max, setBrightness level)
    let cases = [
        (50, 100, 0.4),
        (30, 80, 0.0),
        (200, 255, 0.999),
        (10, 100, 1.0),
        (7, 9, 0.5),
    ];
    for (k, (cur, max, level)) in cases.into_iter().enumerate() {
        let r = run(&d.join(k.to_string()), cur, max, level);
        let v = ((level * max as f64).floor() as u32).max(1);
        // The value just read is written back once `brightness` is set
        // (onMultipliedBrightnessChanged restarts setTimer), then the new level.
        let echo = ((cur as f64 / max as f64 * max as f64).floor() as u32).max(1);
        let set = |x: u32| format!("-b 5 setvcp 10 {x}\n");
        assert_eq!(r.sets, format!("{}{}", set(echo), set(v)), "case {k}");
        assert_eq!(r.vcp, (v, max), "case {k}");
        assert_eq!(
            (r.raw_max, r.brightness, r.ready),
            (max as f64, cur as f64 / max as f64, true),
            "case {k} ({cur}/{max} -> {level})"
        );
    }
}
