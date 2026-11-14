//! The `DisplayBrightness` singleton of the `Impulse` module
//! (`qmltypes/Impulse/display_brightness.qmltypes`): what `services/Brightness.qml` ran
//! `brightnessctl` and `ddcutil` for.
//!
//! `detectDdc()` is `ddcutil detect`, `readBacklight()` / `readDdc()` are `brightnessctl g/m`
//! and `ddcutil getvcp 10`, `setBacklight()` / `setDdc()` are `brightnessctl s` and
//! `ddcutil setvcp 10`; `backlightChanged` reports a level set outside the shell. Backlight
//! goes through `/sys/class/backlight` (logind `SetBrightness` when the file is not writable,
//! as brightnessctl does), DDC/CI as ddcutil talks to it: VESA DDC/CI Get/Set VCP Feature for
//! MCCS code 0x10 on `/dev/i2c-N` (ddc-i2c). Backlight and DDC each run on their own worker
//! thread; results come back to the UI thread in [`poll`] and leave as signals.

use crate::eval::Interp;
use crate::runtime::{Runtime, World};
use crate::value::Value;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

/// brightnessctl's `classes[]`, in its order.
const CLASSES: [&str; 2] = ["backlight", "leds"];
/// VCP feature code of luminance.
pub const VCP_BRIGHTNESS: u8 = 0x10;

// ---- brightnessctl equivalents (pure, over a sysfs root) ----

/// A device as brightnessctl's `read_device` sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub class: String,
    pub id: String,
    pub cur: u32,
    pub max: u32,
}

/// `fscanf(f, "%u")`: `None` at EOF (read error), 0 on no digits (value left as calloc'd).
fn scan_u32(s: &str) -> Option<u32> {
    let t = s.trim_start();
    if t.is_empty() {
        return None;
    }
    let (neg, t) = match t.as_bytes()[0] {
        b'-' => (true, &t[1..]),
        b'+' => (false, &t[1..]),
        _ => (false, t),
    };
    let n = t.bytes().take_while(u8::is_ascii_digit).count();
    let v = t[..n]
        .parse::<u64>()
        .map(|v| v as u32)
        .unwrap_or(if n == 0 { 0 } else { u32::MAX });
    Some(if neg { v.wrapping_neg() } else { v })
}

/// brightnessctl `read_device`: `None` when it reports an error (device skipped).
pub fn read_device(sys: &Path, class: &str, id: &str) -> Option<Device> {
    let dir = sys.join("class").join(class).join(id);
    let mut d = Device {
        class: class.into(),
        id: id.into(),
        cur: 0,
        max: 0,
    };
    for e in std::fs::read_dir(&dir).ok()? {
        let e = e.ok()?;
        let name = e.file_name();
        let cur = name == "brightness";
        if !cur && name != "max_brightness" {
            continue;
        }
        let s = std::fs::read_to_string(dir.join(&name)).ok()?;
        let v = scan_u32(&s)?;
        if cur {
            d.cur = v;
        } else {
            d.max = v;
        }
    }
    Some(d)
}

/// brightnessctl `read_class` (readdir order, unreadable devices skipped).
pub fn read_class(sys: &Path, class: &str, all: bool) -> Vec<Device> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(sys.join("class").join(class)) else {
        return out;
    };
    for e in rd.flatten() {
        if let Some(d) = read_device(sys, class, &e.file_name().to_string_lossy()) {
            out.push(d);
            if !all {
                break;
            }
        }
    }
    out
}

/// `brightnessctl g` / `brightnessctl m` (no class, no device): `read_single_device`.
pub fn default_device(sys: &Path) -> Option<Device> {
    CLASSES.iter().find_map(|c| read_class(sys, c, false).pop())
}

/// brightnessctl `percent_to_val` with the default exponent 1 (float32 math, as in C).
pub fn percent_to_val(percent: f32, max: u32) -> u32 {
    ((percent / 100.0).powf(1.0) * max as f32).round() as u32
}

/// A `brightnessctl s` value: `N%` or a raw `1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SetValue {
    Percent(i64),
    Raw(i64),
}

/// brightnessctl `calc_value` for a direct value with the default `--min-value` 0.
pub fn calc_value(v: SetValue, max: u32) -> u32 {
    let new = match v {
        SetValue::Percent(p) => percent_to_val(p as f32, max) as i64,
        SetValue::Raw(r) => r,
    };
    new.clamp(0, max as i64) as u32
}

/// `setBacklight(fraction)`: whole percent rounded down, and raw 1 instead of 0% so the panel
/// never goes fully black (`None` for NaN: nothing is written).
pub fn backlight_arg(multiplied: f64) -> Option<SetValue> {
    let n = (js_max(multiplied, 0.0) * 100.0).floor();
    if n.is_nan() {
        return None;
    }
    Some(if n == 0.0 {
        SetValue::Raw(1)
    } else {
        SetValue::Percent(n as i64)
    })
}

/// `Math.max` (NaN wins).
fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}

/// `setDdc(bus, fraction, rawMax)`: the VCP 0x10 value, rounded down and at least 1.
pub fn ddc_arg(multiplied: f64, raw_max: f64) -> Option<u16> {
    let v = js_max((js_max(multiplied, 0.0) * raw_max).floor(), 1.0);
    (!v.is_nan()).then_some(v.min(u16::MAX as f64) as u16)
}

/// Sets every `backlight` device like `brightnessctl --class backlight s V --quiet`:
/// sysfs when writable (or root), else `logind(class, id, value)`. Returns the writes made.
pub fn set_backlight(
    sys: &Path,
    v: SetValue,
    logind: &mut dyn FnMut(&str, &str, u32) -> bool,
) -> Vec<(String, u32)> {
    let mut done = Vec::new();
    for d in read_class(sys, "backlight", true) {
        let new = calc_value(v, d.max);
        let file = sys.join("class/backlight").join(&d.id).join("brightness");
        // SAFETY: geteuid(2) takes no arguments and always succeeds.
        let ok = if unsafe { libc::geteuid() } != 0 && !writable(&file) {
            logind(&d.class, &d.id, new)
        } else {
            std::fs::write(&file, new.to_string()).is_ok()
        };
        if ok {
            done.push((d.id, new));
        }
    }
    done
}

fn writable(p: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = std::ffi::CString::new(p.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `c` is a NUL-terminated path, alive for the call.
    unsafe { libc::access(c.as_ptr(), libc::W_OK) == 0 }
}

/// `org.freedesktop.login1.Session.SetBrightness` on the caller's session (brightnessctl's
/// `logind_set_brightness`).
pub struct Logind;

impl Logind {
    pub fn set(&mut self, class: &str, id: &str, v: u32) -> bool {
        let Some(c) = crate::dbus::system() else {
            return false;
        };
        c.call_method(
            Some("org.freedesktop.login1"),
            "/org/freedesktop/login1/session/auto",
            Some("org.freedesktop.login1.Session"),
            "SetBrightness",
            &(class, id, v),
        )
        .is_ok()
    }
}

// ---- ddcutil equivalents ----

/// DRM connectors that can have a DDC/CI monitor, named as `ddcutil detect` prints them
/// (`card1-DP-1`), each with its i2c buses in probe order: the connector's `ddc` link, then its
/// DP-AUX `i2c-N` children. Disconnected connectors have no EDID, so ddcutil lists nothing there.
pub fn ddc_candidates(sys: &Path) -> Vec<(String, Vec<u32>)> {
    let drm = sys.join("class/drm");
    let Ok(rd) = std::fs::read_dir(&drm) else {
        return Vec::new();
    };
    let mut names: Vec<String> = rd
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("card") && n.contains('-'))
        .collect();
    names.sort();
    let bus =
        |n: &std::ffi::OsStr| -> Option<u32> { n.to_str()?.strip_prefix("i2c-")?.parse().ok() };
    names
        .into_iter()
        .filter_map(|n| {
            let d = drm.join(&n);
            if std::fs::read_to_string(d.join("status")).is_ok_and(|s| s.trim() == "disconnected") {
                return None;
            }
            let mut buses: Vec<u32> = std::fs::canonicalize(d.join("ddc"))
                .ok()
                .and_then(|p| p.file_name().and_then(bus))
                .into_iter()
                .collect();
            let mut aux: Vec<u32> = std::fs::read_dir(&d)
                .into_iter()
                .flatten()
                .flatten()
                .filter_map(|e| bus(&e.file_name()))
                .collect();
            aux.sort_unstable();
            for b in aux {
                if !buses.contains(&b) {
                    buses.push(b);
                }
            }
            (!buses.is_empty()).then_some((n, buses))
        })
        .collect()
}

/// One `Display` block of `ddcutil detect --brief` reduced the way the original's SplitParser does:
/// `{name: <DRM connector without card prefix>, busNum}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DdcMonitor {
    pub name: String,
    pub bus: String,
}

/// The original's parse of a `ddcutil detect --brief` block (`None` where the QML would skip or throw).
pub fn parse_detect_block(data: &str) -> Option<DdcMonitor> {
    if !data.starts_with("Display ") {
        return None;
    }
    let lines: Vec<&str> = data.split('\n').map(str::trim).collect();
    let conn = lines.iter().find(|l| l.starts_with("DRM connector:"))?;
    let bus = lines.iter().find(|l| l.starts_with("I2C bus:"))?;
    let name = conn.split('-').skip(1).collect::<Vec<_>>().join("-");
    let bus = bus.split("/dev/i2c-").nth(1)?.to_string();
    Some(DdcMonitor { name, bus })
}

/// The `DdcMonitor` the original derives from connector `card1-DP-1` on bus `5`.
pub fn monitor_from_connector(connector: &str, bus: u32) -> DdcMonitor {
    DdcMonitor {
        name: connector.split('-').skip(1).collect::<Vec<_>>().join("-"),
        bus: bus.to_string(),
    }
}

/// `(current, max)` as the original reads them from `ddcutil getvcp 10 --brief` (`VCP 10 C cur max`).
pub fn vcp_pair(v: &ddc::VcpValue) -> (u32, u32) {
    (v.value() as u32, v.maximum() as u32)
}

/// A connector whose monitor answered DDC/CI, kept while it shows the same EDID (also across
/// an unplug/replug). Connectors that did not answer are not cached: they are probed again.
struct Probe {
    edid: Vec<u8>,
    buses: Vec<u32>,
    /// The bus that answered.
    bus: u32,
}

/// A write sent without reading it back yet.
struct Unverified {
    value: u16,
    /// Writes of `value` still allowed when the read-back differs.
    resends: usize,
    /// When the monitor may take the next command (DDC/CI wait after Set VCP Feature).
    ready_at: std::time::Instant,
}

/// Opens the DDC/CI channel of `/dev/i2c-N` (tests substitute a simulated monitor).
type OpenBus<D> = Box<dyn FnMut(&Path) -> std::io::Result<D> + Send>;

struct Ddc<D = ddc_i2c::I2cDeviceDdc> {
    sys: PathBuf,
    dev: PathBuf,
    open_bus: OpenBus<D>,
    open: HashMap<String, D>,
    hook: Option<DdcHook>,
    /// DRM connector -> its last probe. Answering monitors stay known across unplugging, so
    /// plugging one back in is not probed again; others only while the connector stays connected.
    known: HashMap<String, Probe>,
    /// Bus -> the last write, read back once no newer write is queued ([`Work::idle`]).
    unverified: HashMap<String, Unverified>,
}

/// Wait the DDC/CI protocol requires after a Set VCP Feature before the next command (the
/// `ddc` crate enforces it on the handle; the read-back is scheduled for when it ends).
const SET_VCP_DELAY: std::time::Duration = std::time::Duration::from_millis(
    <ddc::commands::SetVcpFeature as ddc::commands::Command>::DELAY_COMMAND_MS,
);

/// Tries of one DDC/CI exchange (ddcutil retries failed exchanges too). Between tries the
/// `ddc` crate waits the protocol's delay after a failed command, nothing else sleeps.
const DDC_TRIES: usize = 3;

impl Ddc {
    fn new(sys: PathBuf, dev: PathBuf, hook: Option<DdcHook>) -> Self {
        Ddc {
            sys,
            dev,
            open_bus: Box::new(|p| ddc_i2c::from_i2c_device(p)),
            open: HashMap::new(),
            hook,
            known: HashMap::new(),
            unverified: HashMap::new(),
        }
    }
}

impl<D: ddc::Ddc> Ddc<D> {
    /// `ddcutil detect`: probes only connectors without a cached answering monitor (`Probe`).
    fn detect(&mut self) -> Vec<DdcMonitor> {
        // Only buses wired to a live connector: probing every /dev/i2c-* (ddc-hi's enumerate did)
        // took ~1.4 s on arch-pc against ~0.15 s for ddcutil.
        let candidates = ddc_candidates(&self.sys);
        let mut found: Vec<(u32, DdcMonitor)> = Vec::new();
        for (conn, buses) in candidates {
            let edid = std::fs::read(self.sys.join("class/drm").join(&conn).join("edid"))
                .ok()
                .filter(|e| !e.is_empty());
            let cached = edid.as_ref().and_then(|e| {
                self.known
                    .get(&conn)
                    .filter(|p| p.edid == *e && p.buses == buses)
                    .map(|p| p.bus)
            });
            let bus = cached.or_else(|| {
                let b = buses.iter().copied().find(|&b| self.probe(b))?;
                if let Some(edid) = edid {
                    let buses = buses.clone();
                    self.known.insert(
                        conn.clone(),
                        Probe {
                            edid,
                            buses,
                            bus: b,
                        },
                    );
                }
                Some(b)
            });
            if let Some(b) = bus {
                found.push((b, monitor_from_connector(&conn, b)));
            }
        }
        found.sort_by_key(|f| f.0);
        let live = |bus: &String| found.iter().any(|(_, m)| m.bus == *bus);
        self.open.retain(|b, _| live(b));
        self.unverified.retain(|b, _| live(b));
        found.into_iter().map(|(_, m)| m).collect()
    }

    /// Whether a monitor answers DDC/CI on `bus` (ddcutil lists a display as `Display N` only
    /// once DDC communication works); keeps the handle open when it does.
    fn probe(&mut self, bus: u32) -> bool {
        if let Some(h) = self.hook {
            return h(bus, None).is_some();
        }
        let Ok(mut d) = (self.open_bus)(&self.dev.join(format!("i2c-{bus}"))) else {
            return false;
        };
        if d.get_vcp_feature(VCP_BRIGHTNESS).is_err() {
            return false;
        }
        self.open.insert(bus.to_string(), d);
        true
    }

    fn display(&mut self, bus: &str) -> Option<&mut D> {
        if !self.open.contains_key(bus) {
            let d = (self.open_bus)(&self.dev.join(format!("i2c-{bus}"))).ok()?;
            self.open.insert(bus.to_string(), d);
        }
        self.open.get_mut(bus)
    }

    fn get(&mut self, bus: &str) -> Option<(u32, u32)> {
        if let Some(h) = self.hook {
            return h(bus.parse().ok()?, None);
        }
        (0..DDC_TRIES).find_map(|_| {
            let v = self.display(bus)?.get_vcp_feature(VCP_BRIGHTNESS).ok()?;
            Some(vcp_pair(&v))
        })
    }

    /// One Set VCP Feature exchange (`ddcutil setvcp --noverify`), retried only when it fails.
    fn send_once(&mut self, bus: &str, v: u16) -> bool {
        (0..DDC_TRIES).any(|_| match (self.hook, bus.parse()) {
            (Some(h), Ok(b)) => h(b, Some(v)).is_some(),
            (Some(_), Err(_)) => false,
            (None, _) => self
                .display(bus)
                .is_some_and(|d| d.set_vcp_feature(VCP_BRIGHTNESS, v).is_ok()),
        })
    }

    /// Writes `v` without reading it back; the read-back follows once the writes stop
    /// ([`Ddc::verify`]). Monitors do drop writes (seen on arch-pc: one setvcp right after
    /// another was acked but not applied), so the level that stays is checked, once.
    fn set(&mut self, bus: &str, v: u16) -> bool {
        self.write(bus, v, DDC_TRIES - 1)
    }

    fn write(&mut self, bus: &str, value: u16, resends: usize) -> bool {
        let sent = self.send_once(bus, value);
        if sent {
            self.unverified.insert(
                bus.to_string(),
                Unverified {
                    value,
                    resends,
                    ready_at: std::time::Instant::now() + SET_VCP_DELAY,
                },
            );
        } else {
            self.unverified.remove(bus);
        }
        sent
    }

    /// Reads back the last write on `bus`: `Some(true)` it stuck, `Some(false)` it did not and
    /// no resend is left, `None` it was written again (read back on the next idle).
    fn verify(&mut self, bus: &str) -> Option<bool> {
        let u = self.unverified.remove(bus)?;
        if self
            .get(bus)
            .is_some_and(|(cur, _)| cur == u32::from(u.value))
        {
            return Some(true);
        }
        if u.resends == 0 {
            elog::eprintln!(
                "ddc: bus {bus}: monitor did not apply brightness {}",
                u.value
            );
            return Some(false);
        }
        self.write(bus, u.value, u.resends - 1);
        None
    }

    /// Writes `v` and reads it back right away (the `ddccheck` probe).
    fn set_verified(&mut self, bus: &str, v: u16) -> bool {
        if !self.set(bus, v) {
            return false;
        }
        loop {
            match self.verify(bus) {
                Some(ok) => return ok,
                None if !self.unverified.contains_key(bus) => return false,
                None => {}
            }
        }
    }
}

impl Work for Ddc {
    fn run(&mut self, j: Job) -> Done {
        match j {
            Job::Detect => Done::Detect {
                monitors: self.detect(),
            },
            Job::Read { item, bus } => Done::Read {
                item,
                val: self
                    .get(bus.as_deref().unwrap_or_default())
                    .map(|(c, m)| (Some(c), Some(m))),
            },
            Job::WriteDdc { bus, value } => {
                self.set(&bus, value);
                Done::Wrote
            }
            Job::WriteBacklight(_) | Job::Refresh { .. } => Done::Wrote,
            #[cfg(test)]
            Job::Fence => Done::Fence,
        }
    }

    fn idle_at(&self) -> Option<std::time::Instant> {
        self.unverified.values().map(|u| u.ready_at).min()
    }

    fn idle(&mut self) {
        let now = std::time::Instant::now();
        let due: Vec<String> = self
            .unverified
            .iter()
            .filter(|(_, u)| u.ready_at <= now)
            .map(|(b, _)| b.clone())
            .collect();
        for bus in due {
            self.verify(&bus);
        }
    }
}

/// Live check against `ddcutil detect --brief` / `getvcp 10` (`ddccheck` binary): one line per
/// monitor, `bus connector cur max`; with `step`, writes cur±delta, rereads, restores, rereads.
pub fn ddc_probe(step: u16) -> Vec<String> {
    let r = Roots::default();
    let mut d = Ddc::new(r.sys, r.dev, None);
    let mut out = Vec::new();
    for m in d.detect() {
        let Some((cur, max)) = d.get(&m.bus) else {
            out.push(format!("{} {} read-failed", m.bus, m.name));
            continue;
        };
        let mut l = format!("{} {} {cur} {max}", m.bus, m.name);
        if step > 0 {
            let to = if cur + step as u32 <= max {
                cur as u16 + step
            } else {
                (cur as u16).saturating_sub(step)
            };
            let ok = d.set_verified(&m.bus, to);
            let after = d.get(&m.bus);
            let back = d.set_verified(&m.bus, cur as u16);
            let restored = d.get(&m.bus);
            l += &format!(" set {to} ok={ok} read={after:?} restore ok={back} read={restored:?}");
        }
        out.push(l);
    }
    out
}

// ---- workers ----

enum Job {
    Detect,
    Read {
        item: usize,
        bus: Option<String>,
    },
    WriteBacklight(SetValue),
    WriteDdc {
        bus: String,
        value: u16,
    },
    /// Reread the backlight after the watched sysfs files changed; `sent` counts the writes
    /// sent before this request (`State::sent`).
    Refresh {
        sent: u64,
    },
    /// Answered with [`Done::Fence`] once every job sent before it is done (tests).
    #[cfg(test)]
    Fence,
}

enum Done {
    Detect {
        monitors: Vec<DdcMonitor>,
    },
    Read {
        item: usize,
        val: Option<(Option<u32>, Option<u32>)>,
    },
    Wrote,
    /// The watched backlight files changed (inotify).
    Poke,
    /// The backlight level changed outside the shell.
    Changed {
        sent: u64,
        cur: u32,
        max: u32,
    },
    #[cfg(test)]
    Fence,
}

/// Drop writes a later write of the same target supersedes (the original's shared `setProc.exec`
/// replaces a still-running write).
fn coalesce(jobs: Vec<Job>) -> Vec<Job> {
    let mut seen = HashSet::new();
    let mut out: Vec<Job> = jobs
        .into_iter()
        .rev()
        .filter(|j| match j {
            Job::WriteBacklight(_) => seen.insert(String::new()),
            Job::WriteDdc { bus, .. } => seen.insert(bus.clone()),
            Job::Refresh { .. } => seen.insert("\0refresh".into()),
            _ => true,
        })
        .collect();
    out.reverse();
    out
}

/// A worker thread's job runner.
trait Work: Send + 'static {
    fn run(&mut self, j: Job) -> Done;
    /// When [`Work::idle`] is due if no job arrives before.
    fn idle_at(&self) -> Option<std::time::Instant> {
        None
    }
    /// Runs when the queue stayed empty until [`Work::idle_at`].
    fn idle(&mut self) {}
}

/// A worker's job queue, counting the jobs sent and not yet answered (see [`busy`]).
struct Jobs {
    tx: Sender<Job>,
    queued: Arc<AtomicUsize>,
}

impl Jobs {
    fn send(&self, j: Job) -> Result<(), std::sync::mpsc::SendError<Job>> {
        self.queued.fetch_add(1, Ordering::SeqCst);
        self.tx.send(j).inspect_err(|_| {
            self.queued.fetch_sub(1, Ordering::SeqCst);
        })
    }

    fn queued(&self) -> usize {
        self.queued.load(Ordering::SeqCst)
    }
}

/// Starts the worker thread `name` that runs `w` on the jobs sent to the returned queue.
/// Where the system refuses the thread (logged), every send to the queue fails and nothing
/// stays counted as queued: brightness is neither read nor set.
fn worker(name: &str, mut w: impl Work, out: crate::svc::Sender<Done>) -> Jobs {
    use std::sync::mpsc::RecvTimeoutError;
    let (tx, rx) = channel::<Job>();
    let queued = Arc::new(AtomicUsize::new(0));
    let left = queued.clone();
    // Work still due at `idle_at` counts as one more job, so `busy` covers it too.
    let mut held = false;
    let woken = out.clone();
    let mut account = move |w: &dyn Work, done: usize| {
        let hold = w.idle_at().is_some();
        if hold && !held {
            left.fetch_add(1, Ordering::SeqCst);
        }
        left.fetch_sub(done + usize::from(held && !hold), Ordering::SeqCst);
        held = hold;
        // Woken after the answers are queued: whoever saw `busy` before sees it change.
        woken.handle().wake();
    };
    let spawned = std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            loop {
                let j = match w.idle_at() {
                    None => match rx.recv() {
                        Ok(j) => j,
                        Err(_) => return,
                    },
                    Some(at) => {
                        match rx
                            .recv_timeout(at.saturating_duration_since(std::time::Instant::now()))
                        {
                            Ok(j) => j,
                            Err(RecvTimeoutError::Timeout) => {
                                w.idle();
                                account(&w, 0);
                                continue;
                            }
                            Err(RecvTimeoutError::Disconnected) => return,
                        }
                    }
                };
                let mut batch = vec![j];
                batch.extend(rx.try_iter());
                let n = batch.len();
                for j in coalesce(batch) {
                    if out.send(w.run(j)).is_err() {
                        return;
                    }
                }
                account(&w, n);
            }
        });
    if let Err(e) = spawned {
        elog::eprintln!("brightness: cannot start the {name} thread: {e}");
    }
    Jobs { tx, queued }
}

/// Whether a brightness job the shell sent is still unanswered. Every change of it wakes the
/// runtime, so a caller can block on the waker until it turns `false` (tests do).
pub fn busy(w: &crate::runtime::World) -> bool {
    w.brightness
        .as_ref()
        .is_some_and(|s| s.backlight.queued() + s.ddc.queued() > 0)
}

/// Filesystem roots the workers use (tests point them at a fake tree).
#[derive(Clone)]
pub struct Roots {
    pub sys: PathBuf,
    pub dev: PathBuf,
    /// Replaces the logind call (tests).
    pub logind: Option<fn(&str, &str, u32) -> bool>,
    /// Replaces the DDC/CI monitor on i2c bus `bus` (tests): `set` writes VCP 0x10, then (or
    /// with `None`) answers its `(current, max)`; `None` is a bus where no monitor answers.
    pub ddc: Option<DdcHook>,
}

/// See [`Roots::ddc`].
pub type DdcHook = fn(bus: u32, set: Option<u16>) -> Option<(u32, u32)>;

impl Default for Roots {
    fn default() -> Self {
        Roots {
            sys: "/sys".into(),
            dev: "/dev".into(),
            logind: None,
            ddc: None,
        }
    }
}

/// A world's brightness workers and the jobs it is waiting on (in `World::brightness`).
pub struct State {
    backlight: Jobs,
    ddc: Jobs,
    rx: Receiver<Done>,
    /// Ends the uevent thread when dropped.
    _uevents: Option<std::os::fd::OwnedFd>,
    /// `detectDdc()` is unanswered.
    detecting: bool,
    /// The monitor objects whose level is being read.
    reading: HashSet<usize>,
    /// Backlight writes sent so far.
    sent: u64,
    /// [`Done::Fence`] answers received (tests).
    #[cfg(test)]
    fenced: u64,
}

/// The backlight worker: brightnessctl's device choice, writes, and the watch on the device.
struct Backlight {
    sys: PathBuf,
    logind: Option<fn(&str, &str, u32) -> bool>,
    poke: crate::svc::Sender<Done>,
    /// The watched device directory and its inotify watch.
    watch: Option<(PathBuf, notify::INotifyWatcher)>,
    /// Device id -> the level this worker last wrote, so its own write's inotify echo is
    /// not reported as an external change.
    own: HashMap<String, u32>,
}

impl Backlight {
    /// brightnessctl's default device, watched; when another device becomes the default
    /// (hotplug) the watch moves to it.
    fn device(&mut self) -> Option<Device> {
        let d = default_device(&self.sys);
        let dir = d
            .as_ref()
            .map(|d| self.sys.join("class").join(&d.class).join(&d.id));
        if self.watch.as_ref().map(|w| &w.0) != dir.as_ref() {
            self.watch =
                dir.and_then(|dir| watch_backlight(&dir, self.poke.clone()).map(|w| (dir, w)));
        }
        d
    }
}

impl Work for Backlight {
    fn run(&mut self, j: Job) -> Done {
        match j {
            Job::Read { item, .. } => {
                let d = self.device();
                Done::Read {
                    item,
                    val: Some((d.as_ref().map(|d| d.cur), d.map(|d| d.max))),
                }
            }
            Job::WriteBacklight(v) => {
                let hook = self.logind;
                let wrote = set_backlight(&self.sys, v, &mut |c, i, n| match hook {
                    Some(h) => h(c, i, n),
                    None => Logind.set(c, i, n),
                });
                self.own.extend(wrote);
                Done::Wrote
            }
            Job::Refresh { sent } => match self.device() {
                Some(d) if self.own.get(&d.id) != Some(&d.cur) => {
                    self.own.remove(&d.id);
                    Done::Changed {
                        sent,
                        cur: d.cur,
                        max: d.max,
                    }
                }
                _ => Done::Wrote,
            },
            #[cfg(test)]
            Job::Fence => Done::Fence,
            _ => Done::Wrote,
        }
    }
}

impl State {
    fn start(r: &Roots, h: &crate::svc::Handle) -> State {
        let (out, rx) = h.channel();
        // uevents describe the real /sys only; test trees elsewhere have none.
        let uevents = (r.sys == Path::new("/sys"))
            .then(|| watch_uevents(out.clone()))
            .flatten();
        let backlight = worker(
            "ri-backlight",
            Backlight {
                sys: r.sys.clone(),
                logind: r.logind,
                poke: out.clone(),
                watch: None,
                own: HashMap::new(),
            },
            out.clone(),
        );
        let ddc = worker("ri-ddc", Ddc::new(r.sys.clone(), r.dev.clone(), r.ddc), out);
        State {
            backlight,
            ddc,
            rx,
            _uevents: uevents,
            detecting: false,
            reading: HashSet::new(),
            sent: 0,
            #[cfg(test)]
            fenced: 0,
        }
    }
}

/// udev uevents of the `backlight` and `leds` classes: `add`/`remove` (a device appears or
/// goes, so brightnessctl's default device may change) and `change` (the kernel's
/// sysfs-class-backlight ABI: firmware/hotkey level changes are reported as change uevents).
/// Each pokes the UI thread like the inotify watch does. DRM connector hotplug needs no
/// uevent here: the compositor reports it as `monitorsChanged`, which reruns DDC detection. Returns the stop pipe's write end: dropping it ends the thread.
fn watch_uevents(poke: crate::svc::Sender<Done>) -> Option<std::os::fd::OwnedFd> {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    let mut p = [0; 2];
    // SAFETY: `p` is the writable array of two descriptors pipe2(2) fills.
    if unsafe { libc::pipe2(p.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return None;
    }
    // SAFETY: pipe2 just returned this read end, owned by nothing else.
    let stop = unsafe { OwnedFd::from_raw_fd(p[0]) };
    // SAFETY: pipe2 just returned this write end, owned by nothing else.
    let stop_w = unsafe { OwnedFd::from_raw_fd(p[1]) };
    std::thread::Builder::new()
        .name("ri-backlight-uevents".into())
        .spawn(move || {
            let listen = || {
                let ctx = udev::Context::new()?;
                let mut b = udev::MonitorBuilder::new(&ctx)?;
                for c in CLASSES {
                    b.match_subsystem(c)?;
                }
                b.listen()
            };
            let mut socket = match listen() {
                Ok(s) => s,
                Err(e) => {
                    elog::eprintln!("backlight uevents: {e}");
                    return;
                }
            };
            let mut fds = [
                libc::pollfd {
                    fd: socket.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: stop.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            loop {
                // SAFETY: `fds` is a writable array of `fds.len()` pollfds over open descriptors.
                if unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) } < 0 {
                    if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                        continue;
                    }
                    return;
                }
                if fds[1].revents != 0 || fds[0].revents & !libc::POLLIN != 0 {
                    return;
                }
                // The socket is non-blocking: drain every queued event, poke once.
                if socket.by_ref().count() > 0 && poke.send(Done::Poke).is_err() {
                    return;
                }
            }
        })
        .ok()?;
    Some(stop_w)
}

/// inotify on the device's `brightness` (writes from brightnessctl, logind, other tools) and
/// `actual_brightness` (the kernel's own changes, `sysfs_notify`): each change pokes the UI
/// thread, which asks the worker to reread the level. Nothing polls.
fn watch_backlight(dir: &Path, poke: crate::svc::Sender<Done>) -> Option<notify::INotifyWatcher> {
    use notify::Watcher;
    use notify::event::{EventKind, ModifyKind};
    let handler = move |r: notify::Result<notify::Event>| {
        if let Ok(e) = r
            && matches!(
                e.kind,
                EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Any)
            )
        {
            let _ = poke.send(Done::Poke);
        }
    };
    let mut w = notify::INotifyWatcher::new(handler, notify::Config::default())
        .map_err(|e| elog::eprintln!("backlight watch: {e}"))
        .ok()?;
    let mut any = false;
    for f in ["brightness", "actual_brightness"] {
        any |= w
            .watch(&dir.join(f), notify::RecursiveMode::NonRecursive)
            .is_ok();
    }
    any.then_some(w)
}

/// The QML name of the singleton.
const NAME: &str = "DisplayBrightness";

thread_local! {
    /// The roots a test set; None is the real system.
    static ROOTS: RefCell<Option<Roots>> = const { RefCell::new(None) };
}

/// Point this thread's brightness workers at `r` (before the first use).
pub fn set_roots(r: Roots) {
    ROOTS.with(|x| *x.borrow_mut() = Some(r));
}

fn state(w: &mut World) -> &mut State {
    let h = w.waker.handle(crate::svc::Svc::Brightness);
    w.brightness.get_or_insert_with(|| {
        ROOTS.with(|r| State::start(&r.borrow().clone().unwrap_or_default(), &h))
    })
}

fn with_state(it: &mut Interp, f: impl FnOnce(&mut State)) {
    if let Some(w) = crate::runtime::world_of(it) {
        f(state(&mut w.borrow_mut()));
    }
}

/// [`crate::impulse::hooked`] for `method` of the singleton: under a spawn hook no worker
/// starts on the real system, so no backlight or monitor is read or set and `/sys` is not
/// watched. Roots a test set ([`set_roots`]) are not the real system: the workers run on them.
fn hooked(method: &str, args: &[&str]) -> bool {
    ROOTS.with(|r| r.borrow().is_none()) && crate::impulse::hooked(NAME, method, args)
}

/// Emits `signal` of the singleton with `args()` at the main loop's next turn: the answer
/// of a sandboxed call.
fn answer(
    it: &mut Interp,
    signal: &'static str,
    params: &'static [&'static str],
    args: impl FnOnce() -> Vec<Value> + Send + 'static,
) {
    let Some(w) = crate::runtime::world_of(it) else {
        return;
    };
    let w = &mut *w.borrow_mut();
    let Some(&me) = w.singletons.get(NAME) else {
        return;
    };
    crate::impulse::replies(w).send(move |w| crate::native::emit(w, me, signal, params, args()));
}

/// `readBacklight(monitor)` / `readDdc(monitor, bus)`: one read per monitor at a time.
fn read(it: &mut Interp, monitor: Option<&Value>, bus: Option<String>) {
    let Some(&Value::Item(item)) = monitor else {
        return;
    };
    let sandboxed = match &bus {
        Some(bus) => hooked("readDdc", &[bus]),
        None => hooked("readBacklight", &[]),
    };
    if sandboxed {
        // Nothing is read: the answer of a read that failed.
        answer(
            it,
            "levelRead",
            &["monitor", "ok", "current", "max"],
            move || {
                let nan = Value::Num(f64::NAN);
                vec![Value::Item(item), Value::Bool(false), nan.clone(), nan]
            },
        );
        return;
    }
    with_state(it, |s| {
        if s.reading.insert(item) {
            let ddc = bus.is_some();
            let job = Job::Read { item, bus };
            let _ = if ddc {
                s.ddc.send(job)
            } else {
                s.backlight.send(job)
            };
        }
    });
}

fn num(a: &[Value], i: usize) -> f64 {
    a.get(i).map_or(f64::NAN, |v| v.to_number())
}

/// The methods of the `DisplayBrightness` singleton.
const METHODS: &[(&str, crate::eval::NativeFn)] = &[
    ("detectDdc", |it, _, _| {
        if hooked("detectDdc", &[]) {
            // Nothing is probed: no monitor answers DDC.
            answer(it, "ddcDetected", &["monitors"], || {
                vec![Value::array(vec![])]
            });
            return Ok(Value::Undefined);
        }
        with_state(it, |s| {
            if !std::mem::replace(&mut s.detecting, true) {
                let _ = s.ddc.send(Job::Detect);
            }
        });
        Ok(Value::Undefined)
    }),
    ("readBacklight", |it, _, a| {
        read(it, a.first(), None);
        Ok(Value::Undefined)
    }),
    ("readDdc", |it, _, a| {
        let bus = a.get(1).map(|v| v.to_js_string()).unwrap_or_default();
        read(it, a.first(), Some(bus));
        Ok(Value::Undefined)
    }),
    ("setBacklight", |it, _, a| {
        if hooked("setBacklight", &[&Value::Num(num(a, 0)).to_js_string()]) {
            return Ok(Value::Undefined);
        }
        if let Some(v) = backlight_arg(num(a, 0)) {
            with_state(it, |s| {
                s.sent += 1;
                let _ = s.backlight.send(Job::WriteBacklight(v));
            });
        }
        Ok(Value::Undefined)
    }),
    ("setDdc", |it, _, a| {
        let bus = a.first().map(|v| v.to_js_string()).unwrap_or_default();
        let shown = [1, 2].map(|i| Value::Num(num(a, i)).to_js_string());
        if hooked("setDdc", &[&bus, &shown[0], &shown[1]]) {
            return Ok(Value::Undefined);
        }
        if let Some(value) = ddc_arg(num(a, 1), num(a, 2)) {
            with_state(it, |s| {
                let _ = s.ddc.send(Job::WriteDdc { bus, value });
            });
        }
        Ok(Value::Undefined)
    }),
];

pub static TYPE: crate::impulse::Type = crate::impulse::Type {
    name: NAME,
    methods: METHODS,
    init: None,
};

/// A decimal field as a number (`NaN` when missing, as `parseInt` of no text).
fn parse_int(v: Option<u32>) -> f64 {
    v.map_or(f64::NAN, f64::from)
}

/// Finished brightness jobs, as the signals of `DisplayBrightness`.
pub(crate) fn poll(w: &mut World) -> bool {
    let Some(s) = w.brightness.as_mut() else {
        return false;
    };
    let done: Vec<Done> = s.rx.try_iter().collect();
    let me = w.singletons.get(NAME).copied();
    let emit = |w: &mut World, sig: &str, params: &[&str], args: Vec<Value>| {
        if let Some(me) = me {
            crate::native::emit(w, me, sig, params, args);
        }
    };
    let mut changed = false;
    let mut poked = false;
    for d in done {
        match d {
            Done::Detect { monitors } => {
                state(w).detecting = false;
                let list = monitors
                    .iter()
                    .map(|m| {
                        Runtime::js_object(&[
                            ("name", Value::str(&m.name)),
                            ("busNum", Value::str(&m.bus)),
                        ])
                    })
                    .collect();
                emit(w, "ddcDetected", &["monitors"], vec![Value::array(list)]);
                changed = true;
            }
            Done::Read { item, val } => {
                if !state(w).reading.remove(&item) || w.dead.contains(&item) {
                    continue;
                }
                let (cur, max) = val.unwrap_or_default();
                emit(
                    w,
                    "levelRead",
                    &["monitor", "ok", "current", "max"],
                    vec![
                        Value::Item(item),
                        Value::Bool(val.is_some()),
                        Value::Num(parse_int(cur)),
                        Value::Num(parse_int(max)),
                    ],
                );
                changed = true;
            }
            Done::Wrote => {}
            #[cfg(test)]
            Done::Fence => state(w).fenced += 1,
            Done::Poke => poked = true,
            // Dropped when the shell sent a write after asking for this level: that newer
            // write wins.
            Done::Changed { sent, cur, max } => {
                if sent == state(w).sent {
                    emit(
                        w,
                        "backlightChanged",
                        &["current", "max"],
                        vec![Value::Num(f64::from(cur)), Value::Num(f64::from(max))],
                    );
                    changed = true;
                }
            }
        }
    }
    if poked {
        let s = state(w);
        let _ = s.backlight.send(Job::Refresh { sent: s.sent });
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    fn tree(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ri-bright-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn dev(sys: &Path, class: &str, id: &str, cur: &str, max: &str) {
        let d = sys.join("class").join(class).join(id);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("brightness"), cur).unwrap();
        std::fs::write(d.join("max_brightness"), max).unwrap();
    }

    /// `brightnessctl --class backlight s N% --quiet` (N = 1..100) and `s 1` per max,
    /// recorded from brightnessctl e70bc55 on a fake /sys/class (tests/data).
    #[test]
    fn set_matches_recorded_brightnessctl() {
        let t = include_str!("../tests/data/brightnessctl-set.txt");
        let mut rows = 0;
        for l in t.lines() {
            let (max, rest) = l.split_once(':').unwrap();
            let max: u32 = max.parse().unwrap();
            let (pcts, r1) = rest.split_once(" r1=").unwrap();
            for (k, v) in pcts.trim_end_matches(',').split(',').enumerate() {
                let want: u32 = v.parse().unwrap();
                assert_eq!(
                    calc_value(SetValue::Percent(k as i64 + 1), max),
                    want,
                    "max {max} {}%",
                    k + 1
                );
            }
            assert_eq!(
                calc_value(SetValue::Raw(1), max),
                r1.parse::<u32>().unwrap()
            );
            rows += 1;
        }
        assert_eq!(rows, 13);
    }

    /// `brightnessctl g` / `m` device choice, expectations recorded from brightnessctl.
    #[test]
    fn get_matches_brightnessctl_device_choice() {
        let s = tree("get");
        assert_eq!(default_device(&s), None); // "Failed to find a suitable device."
        dev(&s, "leds", "caps", "0", "1");
        assert_eq!(default_device(&s).map(|d| (d.cur, d.max)), Some((0, 1)));
        dev(&s, "backlight", "x", "123\n", "9000\n");
        assert_eq!(
            default_device(&s).map(|d| (d.cur, d.max)),
            Some((123, 9000))
        );
        // An empty `brightness` is an EOF error: the device is skipped.
        dev(&s, "backlight", "x", "", "5");
        assert_eq!(default_device(&s).map(|d| d.id), Some("caps".into()));
    }

    #[test]
    fn set_writes_sysfs_or_calls_logind() {
        let s = tree("set");
        dev(&s, "backlight", "x", "50", "100");
        let mut calls = vec![];
        let w = set_backlight(&s, SetValue::Percent(40), &mut |c, i, v| {
            calls.push((c.to_string(), i.to_string(), v));
            true
        });
        assert_eq!(w, vec![("x".to_string(), 40)]);
        assert!(calls.is_empty());
        assert_eq!(
            std::fs::read_to_string(s.join("class/backlight/x/brightness")).unwrap(),
            "40"
        );
        // SAFETY: geteuid(2) takes no arguments and always succeeds.
        if unsafe { libc::geteuid() } != 0 {
            use std::os::unix::fs::PermissionsExt;
            let f = s.join("class/backlight/x/brightness");
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o444)).unwrap();
            set_backlight(&s, SetValue::Raw(1), &mut |c, i, v| {
                calls.push((c.to_string(), i.to_string(), v));
                true
            });
            assert_eq!(calls, vec![("backlight".into(), "x".into(), 1)]);
            assert_eq!(std::fs::read_to_string(&f).unwrap(), "40");
        }
    }

    /// The arguments the original's syncBrightness builds (`Math.floor`, `"1"` instead of `0%`, NaN).
    #[test]
    fn sync_arguments_match_the_original() {
        assert_eq!(backlight_arg(0.0), Some(SetValue::Raw(1)));
        assert_eq!(backlight_arg(-0.3), Some(SetValue::Raw(1)));
        assert_eq!(backlight_arg(0.009), Some(SetValue::Raw(1)));
        assert_eq!(backlight_arg(0.5), Some(SetValue::Percent(50)));
        assert_eq!(backlight_arg(0.999), Some(SetValue::Percent(99)));
        assert_eq!(backlight_arg(1.0), Some(SetValue::Percent(100)));
        assert_eq!(backlight_arg(f64::NAN), None);
        assert_eq!(ddc_arg(0.0, 100.0), Some(1));
        assert_eq!(ddc_arg(0.555, 100.0), Some(55));
        assert_eq!(ddc_arg(1.0, 100.0), Some(100));
        assert_eq!(ddc_arg(0.5, f64::NAN), None);
    }

    /// The original's SplitParser over real `ddcutil detect --brief` output vs the sysfs connector path.
    #[test]
    fn detect_matches_ddcutil_parse() {
        let out = "Display 1\n   I2C bus:          /dev/i2c-5\n   DRM connector:    card1-DP-1\n   Monitor:          DEL:DELL U2720Q:ABC\n\n\
                   Invalid display\n   I2C bus:          /dev/i2c-7\n   DRM connector:    card1-eDP-1\n\n\
                   Display 2\n   I2C bus:          /dev/i2c-12\n   DRM connector:    card0-HDMI-A-1\n";
        let parsed: Vec<_> = out.split("\n\n").filter_map(parse_detect_block).collect();
        assert_eq!(
            parsed,
            vec![
                monitor_from_connector("card1-DP-1", 5),
                monitor_from_connector("card0-HDMI-A-1", 12)
            ]
        );
        assert_eq!(parsed[1].name, "HDMI-A-1");

        let s = tree("drm");
        let drm = s.join("class/drm");
        let conn = |n: &str, status: &str, ddc: Option<u32>, aux: &[u32]| {
            let d = drm.join(n);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("status"), format!("{status}\n")).unwrap();
            if let Some(b) = ddc {
                std::fs::create_dir_all(s.join(format!("devices/i2c-{b}"))).unwrap();
                std::os::unix::fs::symlink(s.join(format!("devices/i2c-{b}")), d.join("ddc"))
                    .unwrap();
            }
            for b in aux {
                std::fs::create_dir_all(d.join(format!("i2c-{b}"))).unwrap();
            }
        };
        conn("card1-DP-1", "connected", Some(1), &[6]);
        conn("card1-DP-2", "disconnected", Some(2), &[7]);
        conn("card1-HDMI-A-2", "connected", Some(4), &[]);
        conn("card0-eDP-1", "unknown", None, &[12]);
        conn("card1-Writeback-1", "unknown", None, &[]);
        std::fs::create_dir_all(drm.join("card1")).unwrap();
        assert_eq!(
            ddc_candidates(&s),
            vec![
                ("card0-eDP-1".to_string(), vec![12]),
                ("card1-DP-1".to_string(), vec![1, 6]),
                ("card1-HDMI-A-2".to_string(), vec![4]),
            ]
        );
    }

    /// A DDC/CI monitor as VESA DDC/CI 1.1 specifies it, at I2C address 0x37: every host
    /// packet is `0x51, 0x80 | len, payload, checksum` with the checksum XOR-ed from the write
    /// address 0x6E; Get VCP Feature (0x01) is answered with a Get VCP Feature Reply (0x02)
    /// framed `0x6E, 0x80 | len, payload, checksum` (XOR from the virtual host address 0x50);
    /// Set VCP Feature (0x03) has no reply. `drop_sets` writes are acked but not applied.
    #[derive(Default)]
    struct SimMonitor {
        cur: u16,
        max: u16,
        drop_sets: usize,
        addr: u16,
        writes: Vec<Vec<u8>>,
        reply: Vec<u8>,
    }

    struct SimBus(std::sync::Arc<std::sync::Mutex<SimMonitor>>);

    impl i2c::Master for SimBus {
        type Error = std::io::Error;
    }

    impl i2c::Address for SimBus {
        fn set_slave_address(&mut self, addr: u16, tenbit: bool) -> std::io::Result<()> {
            assert!(!tenbit);
            self.0.lock().unwrap().addr = addr;
            Ok(())
        }
    }

    impl i2c::ReadWrite for SimBus {
        fn i2c_write(&mut self, b: &[u8]) -> std::io::Result<()> {
            let m = &mut *self.0.lock().unwrap();
            assert_eq!(m.addr, 0x37);
            m.writes.push(b.to_vec());
            let n = (b[1] & 0x7f) as usize;
            assert_eq!((b[0], b[1] & 0x80, b.len()), (0x51, 0x80, n + 3));
            assert_eq!(b[n + 2], b[..n + 2].iter().fold(0x6e, |c, x| c ^ x));
            let p = &b[2..n + 2];
            m.reply.clear();
            match p {
                [0x01, 0x10] => {
                    let [mh, ml] = m.max.to_be_bytes();
                    let [sh, sl] = m.cur.to_be_bytes();
                    m.reply = vec![0x6e, 0x88, 0x02, 0x00, 0x10, 0x00, mh, ml, sh, sl];
                    let c = m.reply.iter().fold(0x50, |c, x| c ^ x);
                    m.reply.push(c);
                }
                [0x03, 0x10, _, _] if m.drop_sets > 0 => m.drop_sets -= 1,
                [0x03, 0x10, vh, vl] => m.cur = u16::from_be_bytes([*vh, *vl]),
                _ => panic!("unexpected DDC/CI request {p:02x?}"),
            }
            Ok(())
        }

        fn i2c_read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            let m = self.0.lock().unwrap();
            let n = m.reply.len().min(out.len());
            out[..n].copy_from_slice(&m.reply[..n]);
            Ok(n)
        }
    }

    /// `ddcutil detect` / `getvcp 10` / `setvcp 10 60` against a monitor on /dev/i2c-5 that
    /// drops the first write: the bytes on the wire are the ones ddcutil sends (Get VCP 0x10 is
    /// the well-known `6E 51 82 01 10 AC`), and the write is read back and resent.
    #[test]
    fn ddc_speaks_mccs_vcp_10_over_i2c() {
        let d = tree("ddc-i2c");
        let conn = d.join("sys/class/drm/card1-DP-1");
        std::fs::create_dir_all(conn.join("i2c-5")).unwrap();
        std::fs::write(conn.join("status"), "connected\n").unwrap();
        let mon = std::sync::Arc::new(std::sync::Mutex::new(SimMonitor {
            cur: 37,
            max: 100,
            drop_sets: 1,
            ..Default::default()
        }));
        let bus = mon.clone();
        let mut ddc = Ddc {
            sys: d.join("sys"),
            dev: d.join("dev"),
            open_bus: Box::new(move |p: &Path| {
                assert!(p.starts_with(d.join("dev")));
                if p.ends_with("i2c-5") {
                    Ok(ddc_i2c::I2cDdc::new(SimBus(bus.clone())))
                } else {
                    Err(std::io::ErrorKind::NotFound.into())
                }
            }),
            open: HashMap::new(),
            hook: None,
            known: HashMap::new(),
            unverified: HashMap::new(),
        };
        assert_eq!(
            ddc.detect(),
            vec![DdcMonitor {
                name: "DP-1".into(),
                bus: "5".into()
            }]
        );
        assert_eq!(ddc.get("5"), Some((37, 100)));
        assert!(ddc.set_verified("5", 60));
        assert_eq!(ddc.get("6"), None);
        let m = mon.lock().unwrap();
        assert_eq!(m.cur, 60);
        let get = vec![0x51, 0x82, 0x01, 0x10, 0xac];
        let set = vec![0x51, 0x84, 0x03, 0x10, 0x00, 60, 0x94];
        assert_eq!(
            m.writes,
            [&get, &get, &set, &get, &set, &get].map(|w| w.clone())
        );
    }

    /// `ddcutil getvcp 10 --brief` prints `VCP 10 C <cur> <max>`; the original reads fields 3 and 4.
    #[test]
    fn vcp_matches_getvcp_brief() {
        let v = ddc::VcpValue {
            ty: 0,
            mh: 0,
            ml: 100,
            sh: 0,
            sl: 37,
        };
        assert_eq!(vcp_pair(&v), (37, 100));
        let f: Vec<&str> = "VCP 10 C 37 100".split(' ').collect();
        assert_eq!((f[3].parse().unwrap(), f[4].parse().unwrap()), vcp_pair(&v));
    }

    #[test]
    fn later_writes_replace_pending_ones() {
        let jobs = vec![
            Job::WriteBacklight(SetValue::Percent(10)),
            Job::Read { item: 1, bus: None },
            Job::WriteDdc {
                bus: "5".into(),
                value: 3,
            },
            Job::WriteBacklight(SetValue::Percent(20)),
            Job::WriteDdc {
                bus: "6".into(),
                value: 4,
            },
            Job::WriteDdc {
                bus: "5".into(),
                value: 9,
            },
        ];
        let kept: Vec<String> = coalesce(jobs)
            .iter()
            .map(|j| match j {
                Job::WriteBacklight(SetValue::Percent(p)) => format!("bl{p}"),
                Job::WriteDdc { bus, value } => format!("ddc{bus}={value}"),
                Job::Read { .. } => "read".into(),
                _ => "?".into(),
            })
            .collect();
        assert_eq!(kept, ["read", "bl20", "ddc6=4", "ddc5=9"]);
    }

    /// Brightness.qml reduced to the members that call `DisplayBrightness`, verbatim from
    /// qml.
    fn reduced_brightness_qml() -> String {
        let src = include_str!("../../../qml/services/Brightness.qml");
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
             {}\n{}\n{}\n{}\n\
             component BrightnessMonitor: QtObject {{\n id: monitor\n required property ShellScreen screen\n property bool isDdc\n property string busNum\n\
             property int rawMaxBrightness: 100\n property real brightness\n property real brightnessMultiplier: 1.0\n property real multipliedBrightness: brightness\n\
             property bool ready: false\n property var externalLevel: null\n\
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

    fn elems(v: &Value) -> Vec<Value> {
        match v {
            Value::Obj(o) => o.borrow().array.clone().unwrap_or_default(),
            _ => vec![],
        }
    }

    struct Run {
        raw_max: f64,
        brightness: f64,
        ready: bool,
        sysfs: String,
        cmds: Vec<Vec<String>>,
    }

    fn run(sys: &Path) -> Run {
        run_at(sys, 0.4)
    }

    /// `argv` as a process that sees `sys/class` as `/sys/class` (user + mount namespace).
    fn in_fake_sys(sys: &Path, argv: &[String]) -> std::process::Output {
        std::process::Command::new("unshare")
            .args([
                "-Urm",
                "sh",
                "-c",
                "mount --bind \"$0\" /sys/class && exec \"$@\"",
            ])
            .arg(sys.join("class"))
            .args(argv)
            .output()
            .unwrap()
    }

    /// Changes a sysfs attribute the way the kernel's store does: readers see the old value or
    /// the new one, never the empty file `std::fs::write`'s O_TRUNC leaves for a moment (the
    /// worker reads the file whenever its watch fires, for its own writes too).
    fn store(file: &Path, v: &str) {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().write(true).open(file).unwrap();
        assert!(
            v.len() as u64 >= f.metadata().unwrap().len(),
            "a shorter value"
        );
        f.write_all(v.as_bytes()).unwrap();
    }

    /// A level set outside the shell (a `brightnessctl` keybind writing sysfs) reaches the
    /// monitor through inotify and is not written back rounded; the shell's own write does
    /// not bounce back into the slider.
    #[test]
    fn external_backlight_changes_reach_the_monitor() {
        let s = tree("external");
        dev(&s, "backlight", "x", "500", "1000");
        set_roots(Roots {
            sys: s.clone(),
            dev: s.join("dev"),
            logind: None,
            ddc: None,
        });
        let file = s.join("services/Brightness.qml");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, reduced_brightness_qml()).unwrap();
        let wake = crate::testenv::Wakeups::new();
        let mut rt = Runtime::new(None);
        rt.world.borrow().waker.set_ping(wake.ping());
        let root = rt.load_file(&file).unwrap();
        rt.set_screens(&[("DP-1".into(), 0, 0, 100, 100)]);
        let mut t = 0.0;
        let level = |rt: &mut Runtime| {
            let m = elems(&rt.get(root, "monitors")).first().cloned();
            m.map(|m| match m {
                Value::Item(m) => rt.get(m, "brightness").to_number(),
                _ => panic!(),
            })
        };
        until(&mut rt, &wake, &mut t, |rt| level(rt) == Some(0.5));
        // initializeMonitor's own write is done before the level changes outside.
        settle_workers(&mut rt, &wake, &mut t);
        let Value::Item(m) = elems(&rt.get(root, "monitors"))[0] else {
            panic!()
        };
        let sysfs = || std::fs::read_to_string(s.join("class/backlight/x/brightness")).unwrap();
        let sent = |rt: &Runtime| rt.world.borrow().brightness.as_ref().unwrap().sent;

        let before = sent(&rt);
        store(&s.join("class/backlight/x/brightness"), "437");
        until(&mut rt, &wake, &mut t, |rt| level(rt) == Some(0.437));
        // Every timer the change started has run: none of them sent a write.
        drain_timers(&mut rt, &mut t);
        assert_eq!(sent(&rt), before, "the external level was written back");
        assert_eq!(sysfs(), "437");

        let _ = rt.call_method(m, "setBrightness", vec![Value::Num(0.256)]);
        until(&mut rt, &wake, &mut t, |_| sysfs() == "250");
        assert_eq!(rt.get(m, "brightness").to_number(), 0.256);

        settle_workers(&mut rt, &wake, &mut t);
        let before = sent(&rt);
        store(&s.join("class/backlight/x/brightness"), "800\n");
        until(&mut rt, &wake, &mut t, |rt| level(rt) == Some(0.8));
        drain_timers(&mut rt, &mut t);
        assert_eq!(sent(&rt), before, "the external level was written back");
        assert_eq!(sysfs(), "800\n");
        set_roots(Roots::default());
    }

    /// Runs loop turns on the test clock `t` until `done`: a turn that changed something (a
    /// Process event it delivered may have started another Process) is followed by another,
    /// due timers run at once (the clock jumps to them), and otherwise the loop sleeps on the
    /// runtime's wakeups (worker replies, the backlight's inotify watch).
    fn until(
        rt: &mut Runtime,
        wake: &crate::testenv::Wakeups,
        t: &mut f64,
        mut done: impl FnMut(&mut Runtime) -> bool,
    ) {
        loop {
            let changed = turn(rt, *t);
            if done(rt) {
                return;
            }
            let d = rt.next_deadline(*t);
            if d.is_finite() {
                *t += d;
            } else if !changed {
                wake.wait(None, None);
            }
        }
    }

    /// One loop turn at `t`; returns whether the native side changed anything.
    fn turn(rt: &mut Runtime, t: f64) -> bool {
        rt.tick(t);
        let changed = rt.poll_native();
        rt.settle();
        changed
    }

    /// Runs the timers due now and those they start, on the test clock `t`, until a turn
    /// changes nothing and no timer is left.
    fn drain_timers(rt: &mut Runtime, t: &mut f64) {
        loop {
            let changed = turn(rt, *t);
            let d = rt.next_deadline(*t);
            if d.is_finite() {
                *t += d;
            } else if !changed {
                return;
            }
        }
    }

    /// Runs the due timers, then waits until both workers have done every job sent so far
    /// (a [`Job::Fence`] behind them is answered) and applies their answers.
    fn settle_workers(rt: &mut Runtime, wake: &crate::testenv::Wakeups, t: &mut f64) {
        drain_timers(rt, t);
        let fenced = |rt: &Runtime| rt.world.borrow().brightness.as_ref().map(|s| s.fenced);
        let Some(before) = fenced(rt) else {
            return;
        };
        {
            let w = rt.world.borrow();
            let s = w.brightness.as_ref().unwrap();
            s.backlight.send(Job::Fence).unwrap();
            s.ddc.send(Job::Fence).unwrap();
        }
        until(rt, wake, t, |rt| fenced(rt) == Some(before + 2));
        drain_timers(rt, t);
    }

    /// Brightness.qml on the tree `sys`: the monitor once it is ready, then
    /// `setBrightness(level)`. `cmds` are the processes it started (none).
    fn run_at(sys: &Path, level: f64) -> Run {
        set_roots(Roots {
            sys: sys.into(),
            dev: sys.join("dev"),
            logind: None,
            ddc: None,
        });
        let cmds = Rc::new(RefCell::new(Vec::new()));
        let c = cmds.clone();
        let file = sys.join("services/Brightness.qml");
        crate::native::set_spawn_hook(Some(Box::new(move |argv: &[String]| {
            c.borrow_mut().push(argv.to_vec());
            Some(String::new())
        })));
        let wake = crate::testenv::Wakeups::new();
        let mut rt = Runtime::new(None);
        rt.world.borrow().waker.set_ping(wake.ping());
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, reduced_brightness_qml()).unwrap();
        let root = rt.load_file(&file).unwrap();
        rt.set_screens(&[("DP-1".into(), 0, 0, 100, 100)]);
        let mut t = 0.0;
        until(&mut rt, &wake, &mut t, |rt| {
            elems(&rt.get(root, "monitors"))
                .first()
                .is_some_and(|m| matches!(m, Value::Item(m) if rt.get(*m, "ready").truthy()))
        });
        settle_workers(&mut rt, &wake, &mut t);
        let Value::Item(m) = elems(&rt.get(root, "monitors"))[0] else {
            panic!()
        };
        let r = (
            rt.get(m, "rawMaxBrightness").to_number(),
            rt.get(m, "brightness").to_number(),
            rt.get(m, "ready").truthy(),
        );
        let _ = rt.call_method(m, "setBrightness", vec![Value::Num(level)]);
        settle_workers(&mut rt, &wake, &mut t);
        crate::native::set_spawn_hook(None);
        let cmds = cmds.borrow().clone();
        Run {
            raw_max: r.0,
            brightness: r.1,
            ready: r.2,
            sysfs: std::fs::read_to_string(sys.join("class/backlight/x/brightness")).unwrap(),
            cmds,
        }
    }

    /// What the real `ddcutil` and `brightnessctl` do on the tree `sys` with the command lines
    /// Brightness.qml used to run: detect (no DDC/CI monitor), read the level, then one
    /// `brightnessctl s` per change of `brightness` (the level read, then `level`). Returns
    /// `(rawMaxBrightness, brightness)` as read.
    fn real_tools(sys: &Path, level: f64) -> (f64, f64) {
        let out = |argv: &[&str]| {
            let argv: Vec<String> = argv.iter().map(|a| a.to_string()).collect();
            String::from_utf8_lossy(&in_fake_sys(sys, &argv).stdout).into_owned()
        };
        assert!(!out(&["ddcutil", "detect", "--brief"]).contains("Display "));
        let num = |what: &str| out(&["brightnessctl", what]).trim().parse::<f64>().unwrap();
        let (cur, max) = (num("g"), num("m"));
        let mut brightness = 0.0;
        for v in [cur / max, level] {
            if v == brightness {
                continue;
            }
            brightness = v;
            let n = (v.max(0.0) * 100.0).floor();
            let arg = if n == 0.0 {
                "1".to_string()
            } else {
                format!("{n}%")
            };
            out(&[
                "brightnessctl",
                "--class",
                "backlight",
                "s",
                &arg,
                "--quiet",
            ]);
        }
        (max, cur / max)
    }

    /// The real `brightnessctl` and `ddcutil` on a fake `/sys/class` (bind-mounted in a user +
    /// mount namespace: the container needs seccomp, systempaths and apparmor
    /// `--security-opt ...=unconfined`) vs Brightness.qml on a copy of the same tree: same
    /// monitor state and the same sysfs files afterwards. Both sides find no DDC/CI monitor:
    /// the fake tree has a DRM connector with an i2c bus but no device answers on it (a DDC/CI
    /// monitor cannot be faked; `i2c-stub` only emulates SMBus register reads).
    #[test]
    fn matches_real_brightnessctl() {
        for tool in ["unshare", "brightnessctl", "ddcutil"] {
            if !crate::testenv::require(tool) {
                return;
            }
        }
        let probe = tree("real-probe");
        std::fs::create_dir_all(probe.join("class")).unwrap();
        if !in_fake_sys(&probe, &["true".into()]).status.success() {
            crate::testenv::unavailable(
                "no user + mount namespaces (docker run --security-opt seccomp=unconfined --security-opt systempaths=unconfined --security-opt apparmor=unconfined)",
            );
            return;
        }
        let cases = [
            ("4794", "9600", 0.4),
            ("10", "255", 0.0),
            ("0", "1", 0.7),
            ("937", "937", 0.999),
            ("120000", "120000", 1.0),
            ("7", "100", 0.123),
            ("19393", "19393", 0.55),
        ];
        let files = |s: &Path| {
            ["backlight/x/brightness", "leds/kbd/brightness"]
                .map(|f| std::fs::read_to_string(s.join("class").join(f)).unwrap())
        };
        for (k, (cur, max, level)) in cases.into_iter().enumerate() {
            let tree_of = |side: &str| {
                let s = tree(&format!("real-{k}-{side}"));
                dev(&s, "backlight", "x", cur, max);
                dev(&s, "leds", "kbd", "1", "3");
                let conn = s.join("class/drm/card1-DP-1");
                std::fs::create_dir_all(conn.join("i2c-5")).unwrap();
                std::fs::write(conn.join("status"), "connected\n").unwrap();
                s
            };
            let s = tree_of("tools");
            let (raw_max, brightness) = real_tools(&s, level);
            let tools = (raw_max, brightness, true, files(&s));
            let s = tree_of("shell");
            let r = run_at(&s, level);
            assert!(r.cmds.is_empty(), "case {k}: spawned {:?}", r.cmds);
            assert_eq!(
                tools,
                (r.raw_max, r.brightness, r.ready, files(&s)),
                "case {k} ({cur}/{max} -> {level}): the tools vs the shell"
            );
        }
    }

    /// The level is read as `brightnessctl g/m` report it, and `setBrightness(0.4)` writes what
    /// `brightnessctl s 40%` does, with no process.
    #[test]
    fn reads_and_writes_the_backlight() {
        let s = tree("qml");
        dev(&s, "backlight", "x", "4794", "9600");
        let r = run(&s);
        assert!(r.cmds.is_empty(), "spawned {:?}", r.cmds);
        assert_eq!(
            (r.raw_max, r.brightness, r.ready),
            (9600.0, 4794.0 / 9600.0, true)
        );
        assert_eq!(r.sysfs, calc_value(SetValue::Percent(40), 9600).to_string());
    }

    fn svc_channel() -> (crate::svc::Sender<Done>, Receiver<Done>) {
        crate::svc::Waker::default()
            .handle(crate::svc::Svc::Brightness)
            .channel()
    }

    /// `(bus, written value)` per DDC exchange; `None` is a Get VCP Feature.
    type DdcLog = std::sync::Mutex<Vec<(u32, Option<u16>)>>;

    static WRITES_LOG: DdcLog = DdcLog::new(Vec::new());
    static WRITES_VCP: std::sync::Mutex<u16> = std::sync::Mutex::new(0);
    static WRITES_DROP_FIRST: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);
    /// Notified after each exchange [`writes_hook`] answered.
    static WRITES_LOGGED: std::sync::Condvar = std::sync::Condvar::new();

    /// A monitor on bus 5 that acks every write and, while `WRITES_DROP_FIRST`, drops the
    /// first one it gets (acked but not applied, as seen on arch-pc).
    fn writes_hook(bus: u32, set: Option<u16>) -> Option<(u32, u32)> {
        let mut log = WRITES_LOG.lock().unwrap();
        let mut vcp = WRITES_VCP.lock().unwrap();
        if let Some(v) = set
            && !WRITES_DROP_FIRST.swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            *vcp = v;
        }
        log.push((bus, set));
        WRITES_LOGGED.notify_all();
        (bus == 5).then_some((u32::from(*vcp), 100))
    }

    /// Slider steps are plain Set VCP Feature exchanges; the level that stays is read back
    /// once, when the bus is free again (DDC/CI wait after Set VCP), and a dropped write is
    /// sent again. One test drives both the `Ddc` calls and the worker (shared hook state).
    #[test]
    fn ddc_writes_are_read_back_once_after_they_stop() {
        let log = || std::mem::take(&mut *WRITES_LOG.lock().unwrap());
        let mut d = Ddc::new(PathBuf::new(), PathBuf::new(), Some(writes_hook));
        let before = std::time::Instant::now();
        for v in [10, 20, 30] {
            assert!(d.set("5", v));
        }
        assert_eq!(log(), [(5, Some(10)), (5, Some(20)), (5, Some(30))]);
        let due = d.idle_at().unwrap();
        assert!(due >= before + SET_VCP_DELAY);
        assert_eq!(d.verify("5"), Some(true));
        assert_eq!(log(), [(5, None)]);
        assert_eq!(d.idle_at(), None);

        WRITES_DROP_FIRST.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(d.set("5", 40));
        assert_eq!(d.verify("5"), None, "dropped write is sent again");
        assert_eq!(d.verify("5"), Some(true));
        assert_eq!(log(), [(5, Some(40)), (5, None), (5, Some(40)), (5, None)]);
        assert_eq!(*WRITES_VCP.lock().unwrap(), 40);

        // Through the worker: a burst of writes, then the single read-back once it idles.
        let (out, _rx) = svc_channel();
        let tx = worker(
            "test-ddc",
            Ddc::new(PathBuf::new(), PathBuf::new(), Some(writes_hook)),
            out,
        );
        for value in 41..=60 {
            tx.send(Job::WriteDdc {
                bus: "5".into(),
                value,
            })
            .unwrap();
        }
        // The read-back is the worker's idle work: it reports nothing but the exchange itself.
        let mut g = WRITES_LOG.lock().unwrap();
        while !g.iter().any(|e| e.1.is_none()) {
            g = WRITES_LOGGED.wait(g).unwrap();
        }
        drop(g);
        let l = log();
        let reads = l.iter().filter(|e| e.1.is_none()).count();
        assert_eq!(reads, 1, "{l:?}");
        assert_eq!(l.last(), Some(&(5, None)), "{l:?}");
        assert_eq!(l[l.len() - 2], (5, Some(60)), "{l:?}");
        assert!(l.len() <= 21, "{l:?}");
        assert_eq!(*WRITES_VCP.lock().unwrap(), 60);
        drop(tx);
    }

    static PROBES: DdcLog = DdcLog::new(Vec::new());

    /// A DDC/CI monitor answers on bus 5 only.
    fn probe_hook(bus: u32, set: Option<u16>) -> Option<(u32, u32)> {
        PROBES.lock().unwrap().push((bus, set));
        (bus == 5).then_some((50, 100))
    }

    /// `detect` probes a connector only when no monitor with its EDID answered there before: a
    /// hotplug elsewhere, or the same monitor plugged back in, costs no DDC/CI probe.
    #[test]
    fn detect_probes_only_new_monitors() {
        let s = tree("detect-cache");
        let conn = |n: &str, bus: u32, status: &str, edid: &[u8]| {
            let d = s.join("class/drm").join(n);
            std::fs::create_dir_all(d.join(format!("i2c-{bus}"))).unwrap();
            std::fs::write(d.join("status"), status).unwrap();
            std::fs::write(d.join("edid"), edid).unwrap();
        };
        let probes = || std::mem::take(&mut *PROBES.lock().unwrap());
        let mon = |n: &str, b: u32| monitor_from_connector(n, b);
        let mut d = Ddc::new(s.clone(), s.join("dev"), Some(probe_hook));
        conn("card1-DP-1", 5, "connected\n", b"edid-a");
        conn("card1-eDP-1", 7, "connected\n", b"panel");
        assert_eq!(d.detect(), [mon("card1-DP-1", 5)]);
        assert_eq!(probes(), [(5, None), (7, None)]);

        // Another output changing (monitorsChanged) does not probe the answering monitor again.
        // The panel that did not answer (no DDC/CI, or a monitor still waking up) is probed
        // again on every detect, so it is found once it answers.
        assert_eq!(d.detect(), [mon("card1-DP-1", 5)]);
        assert_eq!(probes(), [(7, None)]);

        // Unplug and replug the same monitor: no probe; it is listed again.
        conn("card1-DP-1", 5, "disconnected\n", b"");
        assert_eq!(d.detect(), []);
        conn("card1-DP-1", 5, "connected\n", b"edid-a");
        assert_eq!(d.detect(), [mon("card1-DP-1", 5)]);
        assert_eq!(probes(), [(7, None), (7, None)]);

        // A different monitor on that connector is probed.
        conn("card1-DP-1", 5, "connected\n", b"edid-b");
        assert_eq!(d.detect(), [mon("card1-DP-1", 5)]);
        assert_eq!(probes(), [(5, None), (7, None)]);

        // No EDID to key on: always probed, as before.
        conn("card1-DP-1", 5, "connected\n", b"");
        d.detect();
        d.detect();
        assert_eq!(probes(), [(5, None), (7, None), (5, None), (7, None)]);
        let _ = std::fs::remove_dir_all(&s);
    }

    /// A backlight device that appears later becomes the default device: the next refresh
    /// (udev `add` pokes one) reports its level and moves the inotify watch to it.
    #[test]
    fn hotplugged_backlight_is_followed() {
        let s = tree("hotplug");
        let (poke, rx) = svc_channel();
        let mut b = Backlight {
            sys: s.clone(),
            logind: None,
            poke,
            watch: None,
            own: HashMap::new(),
        };
        let Done::Read { val, .. } = b.run(Job::Read { item: 1, bus: None }) else {
            panic!()
        };
        assert_eq!(val, Some((None, None)));
        assert!(b.watch.is_none());

        dev(&s, "backlight", "intel", "300", "1000");
        let Done::Changed { cur, max, .. } = b.run(Job::Refresh { sent: 0 }) else {
            panic!("new device not reported")
        };
        assert_eq!((cur, max), (300, 1000));
        let watched = s.join("class/backlight/intel");
        assert_eq!(b.watch.as_ref().map(|w| &w.0), Some(&watched));
        std::fs::write(watched.join("brightness"), "400").unwrap();
        assert!(matches!(rx.recv(), Ok(Done::Poke)));

        // The device goes away: the watch is dropped with it.
        std::fs::remove_dir_all(&watched).unwrap();
        assert!(matches!(b.run(Job::Refresh { sent: 0 }), Done::Wrote));
        assert!(b.watch.is_none());
        let _ = std::fs::remove_dir_all(&s);
    }

    /// The system refuses a brightness worker's thread (the limit on threads is reached):
    /// `worker` returns, instead of panicking, a queue that takes no job and counts none as
    /// queued, so `busy` never waits for an answer nobody will send. The refusal is in the
    /// log. In a process of its own: the test reads what that process wrote to its stderr.
    #[test]
    fn a_refused_worker_thread_takes_no_job() {
        const TEST: &str = concat!(module_path!(), "::a_refused_worker_thread_takes_no_job");
        if !crate::testenv::in_own_process(TEST) {
            crate::testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
            return;
        }
        let (out, _rx) = svc_channel();
        crate::testenv::refuse_new_threads();
        let (jobs, log) = crate::testenv::stderr_with(|| {
            worker(
                "test-ddc",
                Ddc::new(PathBuf::new(), PathBuf::new(), Some(writes_hook)),
                out,
            )
        });
        assert!(
            log.contains("brightness: cannot start the test-ddc thread:"),
            "stderr: {log:?}"
        );
        assert!(jobs.send(Job::Detect).is_err());
        assert_eq!(jobs.queued(), 0);
    }
}

/// Cost of one read + one write: the original's processes vs in-process. Needs a brightnessctl built
/// with its sysfs root from `BCTL_SYS` (see the PR): `RI_BRIGHTNESSCTL=... cargo test --features live -p qml
/// --release --lib brightness::bench -- --nocapture`.
#[cfg(test)]
mod bench {
    use super::*;
    use std::process::Command;
    use std::time::Instant;

    #[test]
    #[cfg_attr(
        not(feature = "live"),
        ignore = "live: --features live, docs/benchmarks.md"
    )]
    fn processes_vs_native() {
        let Some(bctl) = std::env::var_os("RI_BRIGHTNESSCTL") else {
            return;
        };
        let sys = std::env::temp_dir().join(format!("ri-bright-bench-{}", std::process::id()));
        let d = sys.join("class/backlight/x");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("brightness"), "500").unwrap();
        std::fs::write(d.join("max_brightness"), "1000").unwrap();
        let n = 300;
        let t = Instant::now();
        for k in 0..n {
            let cmd = format!("echo \"a b c $({0} g) $({0} m)\"", bctl.to_string_lossy());
            let o = Command::new("sh")
                .arg("-c")
                .arg(cmd)
                .env("BCTL_SYS", sys.join("class"))
                .output()
                .unwrap();
            assert!(String::from_utf8_lossy(&o.stdout).starts_with("a b c "));
            let s = Command::new(&bctl)
                .args([
                    "--class",
                    "backlight",
                    "s",
                    &format!("{}%", 1 + k % 100),
                    "--quiet",
                ])
                .env("BCTL_SYS", sys.join("class"))
                .status()
                .unwrap();
            assert!(s.success());
        }
        let procs = t.elapsed() / n;
        let t = Instant::now();
        for k in 0..n {
            assert!(default_device(&sys).is_some());
            let w = set_backlight(
                &sys,
                SetValue::Percent(1 + k as i64 % 100),
                &mut |_, _, _| false,
            );
            assert_eq!(w.len(), 1);
        }
        let native = t.elapsed() / n;
        println!("read+write: processes {procs:?} (4 execs), in-process {native:?} (0 execs)");
    }
}
