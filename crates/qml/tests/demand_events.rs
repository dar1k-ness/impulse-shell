//! The original services that work on events or only while something shows them, instead of polling:
//! Cliphist (through the native `ClipboardHistory`) and, in the QML itself, the stopwatch, the
//! pomodoro, uptime and ResourceUsage.
//!
//! The tests run the runtime's host loop on a virtual clock: between turns they sleep until
//! the runtime's waker or its epoll set (process I/O and exits, inotify) wakes it, or jump the
//! clock to the next timer instead of sleeping to it. The fake `cliphist`s finish a listing
//! only once the test releases their [`Gate`].
use qml::runtime::Runtime;
use qml::value::Value;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::time::Duration;
use testenv::Wakeups;

/// Where the pinned `Date` clock of the TimerService tests starts, in ms since the epoch.
const EPOCH: f64 = 1_700_000_000_000.0;

fn ri() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

/// Whether this is `test`'s own process (`testenv::run_in_own_process`), whose HOME and XDG
/// base directories are under a fresh home (`testenv::home_vars`) and whose
/// `CLIPHIST_DB_PATH` is `db` there. Elsewhere it runs `test` there and returns false.
fn own_process(test: &str) -> bool {
    if testenv::in_own_process(test) {
        return true;
    }
    let home = tempfile::tempdir().unwrap();
    let mut vars = testenv::home_vars(home.path());
    vars.push(("CLIPHIST_DB_PATH", home.path().join("db").into()));
    testenv::run_in_own_process(test, vars);
    false
}

/// The home [`own_process`] made.
fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap())
}

struct Shell {
    rt: Runtime,
    wake: Wakeups,
    ep: Option<i32>,
    /// The virtual time in ms, what the runtime's timers see.
    now: f64,
    /// `Date.now()` follows the virtual clock (`qml::builtins::set_clock`) from [`EPOCH`].
    pinned: bool,
}

/// The original's common singletons and GlobalStates (state isolated in [`own_process`]'s home), then
/// `service`.
fn shell(service: &str) -> (Shell, usize) {
    shell_with(service, false)
}

/// [`shell`], with `Date.now()` on the virtual clock when `pinned`.
fn shell_with(service: &str, pinned: bool) -> (Shell, usize) {
    let mut s = shell_of(&["UpdateClock"], pinned);
    let root =
        s.rt.load_file(&ri().join("services").join(service))
            .unwrap();
    s.rt.settle();
    s.drain();
    (s, root)
}

/// A runtime with the original's singletons, of its services only `services` (created on first use).
fn shell_of(services: &[&str], pinned: bool) -> Shell {
    shell_in(&[], services, pinned)
}

/// [`shell_of`], with the singletons of the directories `more` of the original as well.
fn shell_in(more: &[&str], services: &[&str], pinned: bool) -> Shell {
    let mut s = shell_starting(more, services, pinned);
    s.rt.settle();
    s.drain();
    s
}

/// [`shell_in`] in the turn its singletons are made, with what they started still under
/// way: the shell's start, where the windows are made in that turn.
fn shell_starting(more: &[&str], services: &[&str], pinned: bool) -> Shell {
    if pinned {
        qml::builtins::set_clock(Some(EPOCH));
    }
    for k in ["XDG_CONFIG_HOME", "XDG_STATE_HOME", "XDG_CACHE_HOME"] {
        std::fs::create_dir_all(std::env::var_os(k).unwrap()).unwrap();
    }
    let mut rt = Runtime::new(Some(ri()));
    let wake = Wakeups::new();
    rt.world.borrow().waker.set_ping(wake.ping());
    let ep = rt.process_fd();
    let other: Vec<String> = std::fs::read_dir(ri().join("services"))
        .unwrap()
        .filter_map(|e| Some(e.ok()?.path().file_stem()?.to_str()?.to_string()))
        .filter(|n| !services.contains(&n.as_str()))
        .collect();
    let mut dirs = vec![
        ri(),
        ri().join("modules/common"),
        ri().join("modules/common/functions"),
        ri().join("services"),
    ];
    dirs.extend(more.iter().map(|d| ri().join(d)));
    rt.load_singletons_where(&dirs, &|n| !other.iter().any(|s| s == n));
    Shell {
        rt,
        wake,
        ep,
        now: 0.0,
        pinned,
    }
}

impl Shell {
    fn set_now(&mut self, now: f64) {
        self.now = now;
        if self.pinned {
            qml::builtins::set_clock(Some(EPOCH + now));
        }
    }

    /// Runs loop turns at `now` until nothing is ready: no wakeup or epoll event pending, no
    /// write from the last poll (it counts for the next one) and no timer due.
    fn drain(&mut self) {
        loop {
            let rev = self.rt.revision();
            self.rt.poll_native();
            self.rt.tick(self.now);
            self.rt.poll_native();
            self.rt.settle();
            let woke = self.wake.wait(self.ep, Some(Duration::ZERO));
            if woke.fd {
                self.rt.process_fd_ready();
            }
            if !woke.any() && self.rt.revision() == rev && self.rt.next_deadline(self.now) > 0.0 {
                return;
            }
        }
    }

    /// Sleeps until the runtime is woken (the clock stays); returns whether its epoll set was.
    fn wait_event(&mut self) -> bool {
        let fd = self.wake.wait(self.ep, None).fd;
        if fd {
            self.rt.process_fd_ready();
        }
        fd
    }

    /// Runs the loop until `done`, sleeping for the runtime's events between turns.
    fn until(&mut self, what: &str, mut done: impl FnMut(&mut Self) -> bool) {
        // Named in the output nextest shows when its slow-timeout ends a hang.
        eprintln!("waiting for {what}");
        loop {
            self.drain();
            if done(self) {
                return;
            }
            self.wait_event();
        }
    }

    /// [`Shell::drain`] once the pool's jobs (the timers' reads) are done: before the clock moves
    /// on, all the work of the current instant has landed.
    fn idle(&mut self) {
        loop {
            workpool::wait_idle();
            self.drain();
            if !self.wake.wait(self.ep, Some(Duration::ZERO)).any() {
                return;
            }
        }
    }

    /// Runs the loop through the next `ms` of virtual time, each timer at its deadline.
    fn advance(&mut self, ms: f64) {
        let end = self.now + ms;
        loop {
            self.idle();
            let d = self.rt.next_deadline(self.now);
            if self.now + d > end {
                self.set_now(end);
                self.idle();
                return;
            }
            self.set_now(self.now + d);
        }
    }

    fn read(&mut self, i: usize, n: &str) -> Value {
        self.rt.world.borrow_mut().read(i, n).unwrap_or_default()
    }
    fn num(&mut self, i: usize, n: &str) -> f64 {
        self.read(i, n).to_number()
    }
    fn singleton(&self, n: &str) -> usize {
        self.rt.world.borrow().singletons[n]
    }
    /// The `k`th Timer declared in `root`'s document.
    fn timer(&mut self, root: usize, k: usize) -> usize {
        self.child(root, "Timer", k)
    }
    /// The `k`th `ty` declared in `root`'s document.
    fn child(&mut self, root: usize, ty: &str, k: usize) -> usize {
        let mut w = self.rt.world.borrow_mut();
        let mut ts: Vec<usize> = w
            .of_type(ty)
            .into_iter()
            .filter(|&t| w.items[t].parent == Some(root))
            .collect();
        ts.sort();
        ts[k]
    }
    fn call(&mut self, i: usize, f: &str) {
        self.rt.call_method(i, f, vec![]).unwrap().unwrap();
        self.rt.settle();
    }
}

fn entries(s: &mut Shell, root: usize) -> Vec<String> {
    let t = s.read(root, "entries").to_js_string();
    t.split(',')
        .filter(|e| !e.is_empty())
        .map(str::to_string)
        .collect()
}

/// Whether `cliphist list` runs or its lines are not yet in `entries`.
fn listing(s: &mut Shell) -> bool {
    qml::ports::clip::listing(&s.rt.world.borrow())
}

/// Sets the history tool `ClipboardHistory` runs.
fn set_binary(s: &mut Shell, bin: &Path) {
    let h = s.singleton("ClipboardHistory");
    s.rt.set(h, "binary", Value::str(bin.to_str().unwrap()));
}

/// While held, the fake `cliphist`s wait at `home/gate` (`flock`) before their listing ends;
/// dropping it lets them all through.
struct Gate {
    _locked: File,
}

fn hold(home: &Path) -> Gate {
    let f = File::create(home.join("gate")).unwrap();
    f.lock().unwrap();
    Gate { _locked: f }
}

/// Makes the named pipe `p`.
fn mkfifo(p: &Path) {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(p.as_os_str().as_bytes()).unwrap();
    // SAFETY: mkfifo(3) reads the NUL-terminated path, which outlives the call.
    let r = unsafe { libc::mkfifo(c.as_ptr(), 0o600) };
    assert_eq!(r, 0, "mkfifo {p:?}: {}", std::io::Error::last_os_error());
}

/// Blocks until the process behind `pidfd` exited, without the runtime looking.
fn exited(pidfd: &OwnedFd) {
    Wakeups::new().wait(Some(pidfd.as_raw_fd()), None);
}

/// A `cliphist` whose `list` prints `home/list`, then waits at the [`Gate`] so a change can
/// land mid-listing.
fn fake_cliphist(home: &Path) -> PathBuf {
    let f = home.join("fakecliphist");
    let list = home.join("list");
    testenv::install_script(
        &f,
        &format!(
            "#!/bin/bash\ncat '{}'\nflock '{}' true\n",
            list.display(),
            home.join("gate").display()
        ),
    );
    f
}

fn cliphist(home: &Path) -> (Shell, usize, PathBuf) {
    let db = home.join("db");
    std::fs::write(&db, "x").unwrap();
    assert_eq!(
        std::env::var_os("CLIPHIST_DB_PATH"),
        Some(db.clone().into_os_string())
    );
    std::fs::write(home.join("list"), "1\tone\n").unwrap();
    let (mut s, root) = shell("Cliphist.qml");
    let bin = fake_cliphist(home);
    set_binary(&mut s, &bin);
    s.call(root, "refresh");
    s.until("the first listing", |s| entries(s, root) == ["1\tone"]);
    assert!(!listing(&mut s), "{:?}", s.rt.errors());
    (s, root, db)
}

/// A new history entry (cliphist store writing its database) refreshes the list by itself, with
/// no clipboard signal or `arbitraryRaceConditionDelay` timer involved.
#[test]
fn cliphist_refreshes_when_its_database_changes() {
    if !own_process(concat!(
        module_path!(),
        "::cliphist_refreshes_when_its_database_changes"
    )) {
        return;
    }
    let home = home();
    let (mut s, root, db) = cliphist(&home);
    std::fs::write(home.join("list"), "2\ttwo\n1\tone\n").unwrap();
    std::fs::write(&db, "xy").unwrap();
    s.until("the list refreshed after the database changed", |s| {
        entries(s, root) == ["2\ttwo", "1\tone"]
    });
}

/// A `cliphist` whose `list` reads `home/list` when it starts, writes its pid to the pipe
/// `home/started`, waits at the [`Gate`], then prints that snapshot and exits at once (output
/// and exit reach the shell together, as the real one).
fn snapshot_cliphist(home: &Path) -> PathBuf {
    let f = home.join("snapcliphist");
    testenv::install_script(
        &f,
        &format!(
            "#!/bin/bash\nl=$(cat '{}')\necho $$ 1<>'{}'\nflock '{}' true\nprintf '%s\\n' \"$l\"\n",
            home.join("list").display(),
            home.join("started").display(),
            home.join("gate").display()
        ),
    );
    f
}

/// Starts a listing with the snapshot `cliphist` and puts `list` in place once it has read the
/// old one. The listing waits at the returned gate; the pidfd tells when it exited, so it can
/// exit unobserved and its output and exit are taken in one poll.
fn stale_listing(home: &Path, s: &mut Shell, root: usize, list: &str) -> (Gate, OwnedFd) {
    let bin = snapshot_cliphist(home);
    let started = home.join("started");
    mkfifo(&started);
    // Read-write, so neither this open nor the listings' writes wait for the other end.
    let mut started = BufReader::new(
        File::options()
            .read(true)
            .write(true)
            .open(started)
            .unwrap(),
    );
    set_binary(s, &bin);
    let gate = hold(home);
    s.call(root, "refresh");
    let mut pid = String::new();
    started.read_line(&mut pid).unwrap();
    let pidfd = workpool::pidfd_open(pid.trim().parse().unwrap()).unwrap();
    std::fs::write(home.join("list"), list).unwrap();
    (gate, pidfd)
}

/// A `refresh()` asked while `cliphist list` still runs (the original dropped it, so a change landing
/// during a listing was lost until the next copy) is owed and run after the listing.
#[test]
fn cliphist_refresh_during_a_listing_is_not_lost() {
    if !own_process(concat!(
        module_path!(),
        "::cliphist_refresh_during_a_listing_is_not_lost"
    )) {
        return;
    }
    let home = home();
    let (mut s, root, _db) = cliphist(&home);
    let (gate, _pidfd) = stale_listing(&home, &mut s, root, "3\tthree\n1\tone\n");
    assert!(listing(&mut s));
    s.call(root, "refresh");
    drop(gate);
    s.until("the listings to end", |s| !listing(s));
    assert_eq!(
        entries(&mut s, root),
        ["3\tthree", "1\tone"],
        "refresh asked during a listing was dropped"
    );
}

/// A database change during a listing owes a refresh; the old listing's lines (read with its
/// exit) must not end up in front of the owed listing's.
#[test]
fn cliphist_owed_refresh_does_not_take_the_old_listing() {
    if !own_process(concat!(
        module_path!(),
        "::cliphist_owed_refresh_does_not_take_the_old_listing"
    )) {
        return;
    }
    let home = home();
    let (mut s, root, db) = cliphist(&home);
    let (gate, pidfd) = stale_listing(&home, &mut s, root, "3\tthree\n2\ttwo\n1\tone\n");
    std::fs::write(&db, "xyz").unwrap();
    // The listing waits at the gate having printed nothing, so what turns the epoll set
    // readable is the database watch: the change lands during the listing.
    while !s.wait_event() {
        s.drain();
    }
    s.drain();
    assert!(listing(&mut s));
    drop(gate);
    exited(&pidfd);
    let want = ["3\tthree", "2\ttwo", "1\tone"];
    s.until("the old and the owed listing to end", |s| !listing(s));
    assert_eq!(
        entries(&mut s, root),
        want,
        "owed refresh mixed in the old listing"
    );
}

/// `refresh()` (the IPC `update`) after the listing exited but before the shell has taken its
/// lines: same as above.
#[test]
fn cliphist_refresh_before_the_old_listing_is_delivered() {
    if !own_process(concat!(
        module_path!(),
        "::cliphist_refresh_before_the_old_listing_is_delivered"
    )) {
        return;
    }
    let home = home();
    let (mut s, root, _db) = cliphist(&home);
    let (gate, pidfd) = stale_listing(&home, &mut s, root, "4\tfour\n1\tone\n");
    drop(gate);
    exited(&pidfd);
    s.call(root, "refresh");
    let want = ["4\tfour", "1\tone"];
    s.until("the listings to end", |s| !listing(s));
    assert_eq!(
        entries(&mut s, root),
        want,
        "refresh mixed in the old listing"
    );
}

/// `deleteEntry(entry)` and `wipe()` run the tool with the entry on stdin (`delete`) or
/// `wipe`, and the history is listed again once each has ended.
#[test]
fn cliphist_delete_and_wipe_list_again() {
    if !own_process(concat!(
        module_path!(),
        "::cliphist_delete_and_wipe_list_again"
    )) {
        return;
    }
    let home = home();
    let (mut s, root, _db) = cliphist(&home);
    std::fs::write(home.join("list"), "2\ttwo\n1\tone\n").unwrap();
    s.call(root, "refresh");
    s.until("both entries", |s| entries(s, root).len() == 2);
    // A tool that edits `list` as cliphist edits its database; the database file itself
    // stays as it is, so only the end of `delete`/`wipe` can list again.
    let f = home.join("editcliphist");
    testenv::install_script(
        &f,
        &format!(
            "#!/bin/bash\ncd '{}'\ncase $1 in\nlist) cat list;;\ndelete) grep -vxF \"$(cat)\" list > l; mv l list;;\nwipe) : > list;;\nesac\n",
            home.display()
        ),
    );
    set_binary(&mut s, &f);
    s.rt.call_method(root, "deleteEntry", vec![Value::str("2\ttwo")])
        .unwrap()
        .unwrap();
    s.until("the entry to go", |s| entries(s, root) == ["1\tone"]);
    s.call(root, "wipe");
    s.until("the history to empty", |s| entries(s, root).is_empty());
}

/// The stopwatch's 10 ms timer runs only while the right sidebar shows it; pausing after a
/// hidden stretch still records the elapsed time.
#[test]
fn stopwatch_ticks_only_while_shown() {
    if !own_process(concat!(
        module_path!(),
        "::stopwatch_ticks_only_while_shown"
    )) {
        return;
    }
    let (mut s, root) = shell_with("TimerService.qml", true);
    let gs = s.singleton("GlobalStates");
    let sw = s.timer(root, 1);
    s.call(root, "stopwatchReset");
    s.call(root, "toggleStopwatch");
    s.advance(30.0);
    assert!(s.read(root, "stopwatchRunning").truthy());
    assert!(
        !s.read(sw, "running").truthy(),
        "stopwatch timer runs with the sidebar closed"
    );
    s.rt.set(gs, "sidebarRightOpen", Value::Bool(true));
    s.advance(30.0);
    assert!(s.read(sw, "running").truthy());
    s.rt.set(gs, "sidebarRightOpen", Value::Bool(false));
    s.advance(30.0);
    let before = s.num(root, "stopwatchTime");
    s.advance(300.0);
    s.call(root, "stopwatchRecordLap");
    s.call(root, "stopwatchPause");
    let t = s.num(root, "stopwatchTime");
    assert!(
        t >= before + 29.0,
        "pause kept a stale count: {before} -> {t}"
    );
}

/// The pomodoro wakes at the lap end while hidden and on second boundaries while shown, instead
/// of every 200 ms.
#[test]
fn pomodoro_wakes_at_lap_end_when_hidden() {
    if !own_process(concat!(
        module_path!(),
        "::pomodoro_wakes_at_lap_end_when_hidden"
    )) {
        return;
    }
    let (mut s, root) = shell_with("TimerService.qml", true);
    let gs = s.singleton("GlobalStates");
    let pt = s.timer(root, 0);
    s.call(root, "resetPomodoro");
    s.call(root, "togglePomodoro");
    s.advance(50.0);
    let iv = s.num(pt, "interval");
    assert!(iv > 60_000.0, "hidden pomodoro wakes every {iv} ms");
    s.rt.set(gs, "sidebarRightOpen", Value::Bool(true));
    s.advance(50.0);
    let iv = s.num(pt, "interval");
    assert!((1.0..=1000.0).contains(&iv), "shown pomodoro interval {iv}");
    let left = s.num(root, "pomodoroSecondsLeft");
    let lap = s.num(root, "pomodoroLapDuration");
    assert!(left <= lap && left >= lap - 2.0, "{left} of {lap}");
}

/// Uptime is the original's text for `/proc/uptime`, refreshed when its minute changes.
#[test]
fn uptime_wakes_on_minute_boundaries() {
    if !own_process(concat!(
        module_path!(),
        "::uptime_wakes_on_minute_boundaries"
    )) {
        return;
    }
    let (mut s, root) = shell("DateTime.qml");
    let t = s.timer(root, 0);
    s.advance(100.0);
    let up: f64 = std::fs::read_to_string("/proc/uptime")
        .unwrap()
        .split(' ')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let iv = s.num(t, "interval");
    let next = up * 1000.0 + iv;
    let off = next % 60_000.0;
    assert!(
        !(500.0..59_500.0).contains(&off),
        "next uptime tick {iv} ms away is not on a minute boundary ({off})"
    );
    let text = s.read(root, "uptime").to_js_string();
    let want = [format_uptime(up), format_uptime(up - 1.0)];
    assert!(want.contains(&text), "{text} vs {want:?}");
}

/// DateTime.qml's uptime text for `secs` (`/proc/uptime`'s first field).
fn format_uptime(secs: f64) -> String {
    let (d, h, m) = (
        (secs / 86400.0).floor(),
        ((secs % 86400.0) / 3600.0).floor(),
        ((secs % 3600.0) / 60.0).floor(),
    );
    let mut parts = Vec::new();
    if d > 0.0 {
        parts.push(format!("{d}d"));
    }
    if h > 0.0 {
        parts.push(format!("{h}h"));
    }
    if m > 0.0 || parts.is_empty() {
        parts.push(format!("{m}m"));
    }
    parts.join(", ")
}

/// ResourceUsage samples, and `/proc` is read, only while a resources view is shown: created,
/// hidden, shown and destroyed, each as it happens.
#[test]
fn resources_tick_only_while_shown() {
    if !own_process(concat!(module_path!(), "::resources_tick_only_while_shown")) {
        return;
    }
    let home = home();
    let mut s = shell_of(&["ResourceUsage", "UpdateClock", "SystemInfo"], false);
    let ui = home.join("ui");
    std::fs::create_dir_all(&ui).unwrap();
    let doc = |name: &str, text: &str| {
        let p = ui.join(name);
        std::fs::write(&p, text).unwrap();
        p
    };
    let user = doc(
        "User.qml",
        "import QtQuick\nimport ri.services\nItem { property real total: ResourceUsage.memoryTotal }\n",
    );
    s.rt.load_file(&user).unwrap();
    s.rt.settle();
    let root = s.singleton("ResourceUsage");
    let t = s.child(root, "UpdateTimer", 0);
    let samples = |s: &mut Shell| {
        let h = s.read(root, "memoryUsageHistory").to_js_string();
        h.split(',').filter(|x| !x.is_empty()).count()
    };
    s.advance(10_000.0);
    assert!(!s.read(t, "running").truthy(), "ticks with nothing shown");
    assert_eq!(
        s.num(root, "memoryTotal"),
        1.0,
        "read /proc with nothing shown"
    );

    // A view created later starts it, and its first tick comes at once.
    let bar = doc(
        "Bar.qml",
        "import QtQuick\nimport ri.modules.rusty.bar\nItem { Item { Resources {} } }\n",
    );
    // The vertical bar's widget, through a qualified import as the panel uses it.
    let vbar = doc(
        "VBar.qml",
        "import QtQuick\nimport ri.modules.rusty.verticalBar as VBar\nItem { VBar.Resources {} }\n",
    );
    let view = s.rt.load_file(&bar).unwrap();
    s.advance(10.0);
    assert!(s.read(t, "running").truthy(), "a view was created");
    assert!(s.num(root, "memoryTotal") > 1.0, "no /proc read at once");
    assert_eq!(samples(&mut s), 1);
    // Then one tick per `updateInterval` (3 s); a second view shown meanwhile adds none.
    let more = s.rt.load_file(&vbar).unwrap();
    s.advance(100.0);
    assert_eq!(s.num(root, "users"), 2.0);
    s.rt.set(more, "visible", Value::Bool(false));
    s.advance(100.0);
    assert_eq!(s.num(root, "users"), 1.0);
    assert_eq!(samples(&mut s), 1, "a sample off the interval");
    s.advance(5800.0);
    assert_eq!(samples(&mut s), 3);

    // Hidden through an ancestor: no tick, so no read and no history.
    s.rt.set(view, "visible", Value::Bool(false));
    s.advance(10_000.0);
    assert!(!s.read(t, "running").truthy(), "ticks while hidden");
    assert_eq!(samples(&mut s), 3, "history grew while hidden");

    // Shown again: fresh values at once.
    s.rt.set(view, "visible", Value::Bool(true));
    s.rt.set(more, "visible", Value::Bool(true));
    s.advance(10.0);
    assert!(s.read(t, "running").truthy(), "the view is shown again");
    assert_eq!(samples(&mut s), 4);

    // The last view destroyed stops it.
    s.rt.world.borrow_mut().doom(more);
    s.advance(10.0);
    assert!(s.read(t, "running").truthy(), "one view is left");
    s.rt.world.borrow_mut().doom(view);
    s.advance(10_000.0);
    assert!(
        !s.read(t, "running").truthy(),
        "ticks with its view destroyed"
    );
    assert_eq!(samples(&mut s), 4);
    let errs = s.rt.errors();
    assert!(errs.is_empty(), "{errs:?}");
}

/// The bar's first frame has the memory in use: ResourceUsage's first poll comes in the turn
/// the bar is made, at the shell's start before `/proc/meminfo` has been read, and with no
/// text it took the memory and the swap for full until the next poll.
#[test]
fn resources_first_poll_has_read_the_files() {
    const TEST: &str = concat!(module_path!(), "::resources_first_poll_has_read_the_files");
    if !own_process(TEST) {
        return;
    }
    let mut s = shell_starting(&[], &["ResourceUsage", "UpdateClock", "SystemInfo"], false);
    let ui = home().join("ui");
    std::fs::create_dir_all(&ui).unwrap();
    let bar = ui.join("Bar.qml");
    std::fs::write(
        &bar,
        "import QtQuick\nimport ri.modules.rusty.bar\nItem { Item { Resources {} } }\n",
    )
    .unwrap();
    s.rt.load_file(&bar).unwrap();
    s.advance(10.0);
    let root = s.singleton("ResourceUsage");
    assert_eq!(
        s.read(root, "memoryUsageHistory")
            .to_js_string()
            .split(',')
            .count(),
        1,
        "one poll"
    );
    assert!(s.num(root, "memoryTotal") > 1.0, "polled with no text");
    assert!(s.num(root, "memoryUsedPercentage") < 1.0, "memory full");
    let errs = s.rt.errors();
    assert!(errs.is_empty(), "{errs:?}");
}

/// The overlay's Resources widget (`overlay/resources/Resources.qml`) has a tab per resource
/// and shows the last sample of the chosen one as a percentage. The tabs come from its
/// `property list<var> resources`, a list of its own under the name of QQuickItem's.
#[test]
fn overlay_resources_widget_has_its_tabs_and_numbers() {
    const TEST: &str = concat!(
        module_path!(),
        "::overlay_resources_widget_has_its_tabs_and_numbers"
    );
    if !own_process(TEST) {
        return;
    }
    let home = home();
    let mut s = shell_in(
        &["modules/rusty/overlay"],
        &["ResourceUsage", "UpdateClock", "SystemInfo", "Translation"],
        false,
    );
    let ui = home.join("ui");
    std::fs::create_dir_all(&ui).unwrap();
    let main = ui.join("Main.qml");
    std::fs::write(
        &main,
        "import QtQuick\nimport ri.modules.rusty.overlay.resources\nItem {\n    width: 800; height: 600\n    \
         Resources { modelData: ({ identifier: \"resources\", materialSymbol: \"browse_activity\" }) }\n}\n",
    )
    .unwrap();
    s.rt.load_file(&main).unwrap();
    // The overlay is opened, which shows the widget: the first sample comes at once.
    let states = s.singleton("GlobalStates");
    s.rt.set(states, "overlayOpen", Value::Bool(true));
    s.advance(10.0);

    let texts = |s: &mut Shell, ty: &str, prop: &str| -> Vec<String> {
        let mut w = s.rt.world.borrow_mut();
        let mut of = w.of_type(ty);
        of.sort();
        of.into_iter()
            .map(|i| w.read(i, prop).unwrap_or_default().to_js_string())
            .collect()
    };
    assert_eq!(
        texts(&mut s, "SecondaryTabButton", "buttonText"),
        ["CPU", "RAM", "Swap"]
    );
    let shown: Vec<String> = texts(&mut s, "StyledText", "text")
        .into_iter()
        .filter(|t| t.ends_with('%'))
        .collect();
    assert_eq!(shown.len(), 1, "{shown:?}");
    let percent: f64 = shown[0]
        .trim_end_matches('%')
        .parse()
        .unwrap_or_else(|_| panic!("not a number: {shown:?}"));
    assert!((0.0..=100.0).contains(&percent), "{shown:?}");
    // The other tabs show their own resource: each a number as well.
    let bar = s.rt.world.borrow_mut().of_type("SecondaryTabBar")[0];
    for tab in [1.0, 2.0] {
        s.rt.set(bar, "currentIndex", Value::Num(tab));
        s.advance(10.0);
        let shown: Vec<String> = texts(&mut s, "StyledText", "text")
            .into_iter()
            .filter(|t| t.ends_with('%'))
            .collect();
        assert!(
            shown.len() == 1 && shown[0].trim_end_matches('%').parse::<f64>().is_ok(),
            "tab {tab}: {shown:?}"
        );
    }
    let errs = s.rt.errors();
    assert!(errs.is_empty(), "{errs:?}");
}
