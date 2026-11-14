//! Headless notification flood over the compiled QML runtime, for A/B CPU measurements without a
//! compositor or renderer in the loop (`bench/vflood.sh` is too noisy for small steps).
//! usage: riflood ROOT [N=300] [HIST=50] [MODULES=notificationPopup/NotificationPopup.qml]
//! Prints the runtime thread's CPU ticks (utime+stime) from first Notify until all N are listed
//! and the animations they started have finished.
use qml::headless;
use qml::value::Value;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
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
    // SAFETY: still single-threaded (init_log only installs a subscriber).
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
    // Like the shell's frame loop, on a virtual clock (see `headless`): native poll, then
    // animations and timers, so remove transitions finish and their delegates go away.
    // Frames at 60 Hz like the shell's frame callbacks: views apply model changes at their
    // window's polish (RI_DEBUG=nopolish: on every change).
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
    let mut drv = headless::Driver::new(false);
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
    let o0 = rt.world.borrow().items.len();
    let b0 = qml::runtime::prof_counts();
    let t0 = Instant::now();
    let c0 = ticks();
    let p0 = stat_ticks("/proc/self/stat");
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
        "riflood N={n} HIST={hist}: cpu {} ticks (process {}), wall {} ms, listed {}, done {}, objects/notif {:.1}",
        c1 - c0,
        p1 - p0,
        t0.elapsed().as_millis(),
        listed(&rt, svc),
        ok as u8,
        (rt.world.borrow().items.len() - o0) as f64 / n.max(1) as f64
    );
    if let Some(path) = std::env::var_os("RI_DUMP") {
        use std::fmt::Write;
        let w = rt.world.borrow();
        let mut s = String::new();
        for i in 0..w.items.len() {
            if w.dead.contains(&i) {
                continue;
            }
            let Some(it) = w.items.get(i) else { continue };
            let mut ps: Vec<_> = it
                .props
                .iter()
                .map(|(k, p)| {
                    let v = match &p.value {
                        qml::value::Value::Obj(o) => {
                            let o = o.borrow();
                            format!(
                                "obj arr {:?} keys {}",
                                o.array.as_ref().map(|a| a.len()),
                                o.keys.len()
                            )
                        }
                        v => format!("{v:?}"),
                    };
                    (k.as_str().to_string(), v, p.binding.is_some(), p.deps.len())
                })
                .collect();
            ps.sort();
            let _ = writeln!(
                s,
                "{i} {:?} parent {:?} children {:?}",
                it.types.first().map(|t| t.as_str()),
                it.parent,
                it.children
            );
            for p in ps {
                let _ = writeln!(s, "  {p:?}");
            }
        }
        std::fs::write(path, s).unwrap();
    }
    if qml::log_on!("ri::prof") {
        let b1 = qml::runtime::prof_counts();
        let per = |a: u64, b: u64| (a - b) as f64 / n.max(1) as f64;
        println!(
            "bindings/notif {:.1}, handlers/notif {:.1}",
            per(b1.0, b0.0),
            per(b1.1, b0.1)
        );
    }
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
