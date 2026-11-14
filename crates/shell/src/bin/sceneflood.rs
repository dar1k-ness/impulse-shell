//! Headless notification flood over the compiled QML runtime, for A/B CPU measurements without a
//! compositor or renderer in the loop (`bench/vflood.sh` is too noisy for small steps).
//! sceneflood: riflood plus the scene layout of the popup window each frame, no GPU.
//! usage: sceneflood ROOT [N=300] [HIST=50] [MODULES=notificationPopup/NotificationPopup.qml]
//! Prints the runtime thread's CPU ticks (utime+stime) from first Notify until all N are listed
//! and the animations they started have finished.
use qml::headless;
use qml::value::Value;
use std::cell::Cell;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::time::Instant;

struct Bus(Child);
impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn bus(tmp: &Path) -> Bus {
    let cfg = tmp.join("bus.conf");
    std::fs::write(
        &cfg,
        "<busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen><auth>EXTERNAL</auth>\
         <policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/>\
         <allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>",
    )
    .unwrap();
    let mut c = Command::new("dbus-daemon")
        .arg(format!("--config-file={}", cfg.display()))
        .args(["--nofork", "--nopidfile", "--print-address=1"])
        .stdout(Stdio::piped())
        .spawn()
        .expect("dbus-daemon");
    let mut addr = String::new();
    BufReader::new(c.stdout.take().unwrap())
        .read_line(&mut addr)
        .unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "before the threads that read the environment start"
    )]
    // SAFETY: single-threaded at this point.
    unsafe {
        std::env::set_var("DBUS_SESSION_BUS_ADDRESS", addr.trim())
    };
    Bus(c)
}

fn ticks() -> u64 {
    stat_ticks("/proc/thread-self/stat")
}

fn thread_ns() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `ts` is a live timespec clock_gettime only writes.
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

fn stat_ticks(path: &str) -> u64 {
    let s = std::fs::read_to_string(path).unwrap();
    let f: Vec<&str> = s.rsplit_once(')').unwrap().1.split_whitespace().collect();
    f[11].parse::<u64>().unwrap() + f[12].parse::<u64>().unwrap()
}

fn notify(summary: &str) -> Command {
    let mut c = Command::new("gdbus");
    c.args([
        "call",
        "--session",
        "--dest",
        "org.freedesktop.Notifications",
        "--object-path",
        "/org/freedesktop/Notifications",
        "--method",
        "org.freedesktop.Notifications.Notify",
        "flood",
        "0",
        "",
        summary,
        "body text",
        "[]",
        "{}",
        "int32 -1",
    ]);
    c.stdout(Stdio::null());
    c
}

fn listed(rt: &qml::runtime::Runtime, svc: usize) -> usize {
    match rt.world.borrow_mut().read(svc, "list") {
        Some(Value::Obj(o)) => o.borrow().array.as_ref().map_or(0, Vec::len),
        _ => 0,
    }
}

fn main() {
    qml::debug::init_log();
    let a: Vec<String> = std::env::args().collect();
    let root = PathBuf::from(&a[1]);
    let n: usize = a.get(2).and_then(|v| v.parse().ok()).unwrap_or(300);
    let hist: usize = a.get(3).and_then(|v| v.parse().ok()).unwrap_or(50);
    let modules = a
        .get(4)
        .cloned()
        .unwrap_or_else(|| "notificationPopup/NotificationPopup.qml".into());
    let tmp = std::env::temp_dir().join(format!("riflood-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    #[expect(
        clippy::disallowed_methods,
        reason = "before the threads that read the environment start"
    )]
    // Fresh HOME: default config and an empty notification history every run.
    // SAFETY: single-threaded at this point.
    unsafe {
        std::env::set_var("HOME", &tmp)
    };
    let _bus = bus(&tmp);

    let mut rt = qml::runtime::Runtime::new(Some(root.clone()));
    rt.eng()
        .set_compiled(Some(std::rc::Rc::new(ricompiled::Compiled::new(&root))));
    let mut dirs = vec![
        root.join("modules/common"),
        root.join("modules/common/functions"),
        root.join("modules/common/utils"),
        root.join("services"),
        root.clone(),
    ];
    for fam in ["modules/rusty", "modules/waffle"] {
        if let Ok(rd) = std::fs::read_dir(root.join(fam)) {
            let mut d: Vec<_> = rd
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect();
            d.sort();
            dirs.extend(d);
        }
    }
    rt.load_singletons(&dirs);
    rt.set_screens(&[("Virtual-1".into(), 0, 0, 1920, 1080)]);
    for m in modules.split(',').filter(|m| !m.is_empty()) {
        if let Err(e) = rt.load_file(&root.join("modules/rusty").join(m)) {
            elog::eprintln!("load {m}: {e}");
        }
    }
    if let Err(e) = rt.load_file_without(&root.join("shell.qml"), &["PanelFamilyLoader"]) {
        elog::eprintln!("load shell.qml: {e}");
    }
    let svc = rt
        .world
        .borrow()
        .singletons
        .get("Notifications")
        .copied()
        .expect("Notifications");
    // Like the shell's frame loop, on a virtual clock (see `qml::headless`): native poll, then
    // animations and timers, so remove transitions finish and their delegates go away. Frames
    // at 60 Hz like the shell's frame callbacks: views apply model changes at their window's
    // polish (RI_DEBUG=nopolish: on every change). After each turn the popup windows' scene
    // is laid out again when the layout changed, as the shell does before it draws.
    if !qml::debug::on(qml::debug::Switch::NOPOLISH) {
        let wins: Vec<usize> = {
            let mut w = rt.world.borrow_mut();
            ["PanelWindow", "FloatingWindow", "PopupWindow"]
                .iter()
                .flat_map(|t| w.of_type(t))
                .collect()
        };
        rt.set_polish_wins(wins);
    }
    let wins = shell::scene::find_windows(&rt, "rustshell:notificationPopup");
    assert!(!wins.is_empty(), "no notificationPopup window");
    let scene_ns = Rc::new(Cell::new(0u64));
    let mut drv = {
        let mut fonts = shell::scene::Fonts::default();
        let mut laid = u64::MAX;
        let scene_ns = scene_ns.clone();
        headless::Driver::new(false).after_turn(move |rt| {
            let rev = rt.world.borrow().layout_rev;
            if rev == laid {
                return;
            }
            let t = thread_ns();
            for &w in &wins {
                let size = [
                    rt.get(w, "width").to_number(),
                    rt.get(w, "height").to_number(),
                ];
                shell::scene::layout_window(rt, &mut fonts, w, size);
            }
            scene_ns.set(scene_ns.get() + thread_ns() - t);
            laid = rt.world.borrow().layout_rev;
        })
    };
    drv.run(&mut rt, 1500.0);
    if n + hist > 0 {
        // The server owns its name once its D-Bus thread has registered it.
        let mut c = headless::Children::default();
        let mut wait = Command::new("gdbus");
        wait.args(["wait", "--session", "org.freedesktop.Notifications"]);
        c.spawn(wait);
        c.drain(&mut drv, &mut rt, 0);
        assert_eq!(c.ok, 1, "org.freedesktop.Notifications never appeared");
    }
    let mut history = headless::Children::default();
    for i in 0..hist {
        history.spawn(notify(&format!("h{i}")));
        history.drain(&mut drv, &mut rt, 0);
        drv.turn(&mut rt);
    }
    drv.until(&mut rt, &[], |rt| listed(rt, svc) >= history.ok);
    drv.run(&mut rt, 500.0);

    let base = listed(&rt, svc);
    let t0 = Instant::now();
    let c0 = ticks();
    let p0 = stat_ticks("/proc/self/stat");
    let s0 = scene_ns.get();
    // notify-send style: one short-lived client per notification, a few in flight at once.
    let mut flood = headless::Children::default();
    for i in 0..n {
        flood.spawn(notify(&format!("n{i}")));
        flood.drain(&mut drv, &mut rt, 3);
        drv.turn(&mut rt);
    }
    flood.drain(&mut drv, &mut rt, 0);
    drv.until(&mut rt, &[], |rt| listed(rt, svc) >= base + flood.ok);
    let ok = flood.ok == n;
    // Include the tail: transitions and timers the flood started, until the runtime idles
    // (at most 10 s of virtual time: repeating timers never let it).
    drv.settle(&mut rt, 10000.0);
    let c1 = ticks();
    let p1 = stat_ticks("/proc/self/stat");
    println!(
        "sceneflood N={n} HIST={hist}: cpu {} ticks (process {}, scene {} ms), wall {} ms, listed {}, done {}",
        c1 - c0,
        p1 - p0,
        (scene_ns.get() - s0) / 1_000_000,
        t0.elapsed().as_millis(),
        listed(&rt, svc),
        ok as u8
    );
    if std::env::var("RIFLOOD_DEBUG").is_ok() {
        let w = rt.world.borrow_mut().read(svc, "list");
        elog::eprintln!("list = {w:?}");
        let e = rt.world.borrow().errors.clone();
        elog::eprintln!(
            "errors {}: {:?}",
            e.len(),
            e.iter().take(5).collect::<Vec<_>>()
        );
    }
    let _ = std::fs::remove_dir_all(&tmp);
}
