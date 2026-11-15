//! Headless full-shell scenarios over the compiled QML runtime, for low-noise A/B of runtime CPU
//! (no compositor, no renderer). usage: riscen ROOT hotplug|config [N]
//! Prints the runtime thread's CPU ticks (utime+stime) over the scenario.
use qml::headless;
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
    let s = std::fs::read_to_string("/proc/thread-self/stat").unwrap();
    let f: Vec<&str> = s.rsplit_once(')').unwrap().1.split_whitespace().collect();
    f[11].parse::<u64>().unwrap() + f[12].parse::<u64>().unwrap()
}

fn main() {
    qml::debug::init_log();
    let a: Vec<String> = std::env::args().collect();
    let root = PathBuf::from(&a[1]);
    let scen = a.get(2).cloned().unwrap_or_else(|| "hotplug".into());
    let n: usize = a.get(3).and_then(|v| v.parse().ok()).unwrap_or(30);
    let tmp = std::env::temp_dir().join(format!("riscen-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    if let Ok(src) = std::env::var("RISCEN_CONFIG") {
        let d = tmp.join(".config/impulse-shell");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::copy(src, d.join("config.json")).unwrap();
    }
    #[expect(
        clippy::disallowed_methods,
        reason = "before the threads that read the environment start"
    )]
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
    if let Err(e) = rt.load_file(&root.join("shell.qml")) {
        elog::eprintln!("load shell.qml: {e}");
    }
    let mut drv = headless::Driver::new(!qml::debug::on(qml::debug::Switch::NOPOLISH));
    drv.run(&mut rt, 4000.0);
    let cfg = tmp.join(".config/impulse-shell/config.json");
    let o0 = rt.world.borrow().items.len();
    let b0 = qml::runtime::prof_counts();
    let t0 = Instant::now();
    let c0 = ticks();
    match scen.as_str() {
        "hotplug" => {
            let base = ("Virtual-1".to_string(), 0, 0, 1920, 1080);
            for i in 0..n {
                let extra = (format!("HP{i}"), 1920, 0, 1280, 720);
                rt.set_screens(&[base.clone(), extra]);
                drv.run(&mut rt, 400.0);
                rt.set_screens(std::slice::from_ref(&base));
                drv.run(&mut rt, 400.0);
            }
        }
        "config" => {
            let orig = std::fs::read_to_string(&cfg).expect("config.json");
            for i in 1..=n {
                let py = format!(
                    "import json,sys;c=json.load(open(sys.argv[1]));c['bar']['bottom']={b};c['bar']['cornerStyle']={s};\
                     c['appearance']['transparency']['enable']={t};json.dump(c,open(sys.argv[2],'w'),indent=4)",
                    b = if i % 2 == 1 { "True" } else { "False" },
                    s = i % 3,
                    t = if i % 4 < 2 { "True" } else { "False" },
                );
                let tmpf = tmp.join("cfg.tmp");
                std::fs::write(tmp.join("orig.json"), &orig).unwrap();
                let st = Command::new("python3")
                    .args(["-c", &py])
                    .arg(tmp.join("orig.json"))
                    .arg(&tmpf)
                    .status()
                    .unwrap();
                assert!(st.success());
                // The watch's event is in the inotify queue once rename(2) returns.
                let seen = rt.file_events();
                std::fs::rename(&tmpf, &cfg).unwrap();
                drv.until(&mut rt, &[], |rt| rt.file_events() > seen);
                drv.run(&mut rt, 1000.0);
            }
            let seen = rt.file_events();
            std::fs::write(&cfg, &orig).unwrap();
            drv.until(&mut rt, &[], |rt| rt.file_events() > seen);
            drv.run(&mut rt, 1500.0);
        }
        "panels" => {
            let gs = *rt
                .world
                .borrow()
                .singletons
                .get("GlobalStates")
                .expect("GlobalStates");
            let flags_env = std::env::var("RISCEN_FLAGS").unwrap_or_else(|_| {
                "sidebarRightOpen,sidebarLeftOpen,overviewOpen,sessionOpen,mediaControlsOpen,wallpaperSelectorOpen".into()
            });
            let flags: Vec<String> = flags_env.split(',').map(String::from).collect();
            let sample = |rt: &mut qml::runtime::Runtime, i: usize| {
                rt.reap();
                rt.world.borrow_mut().purge_dead();
                let freed = qml::eval::collect_cycles();
                let w = rt.world.borrow();
                let live = (0..w.items.len()).filter(|i| !w.dead.contains(i)).count();
                let deps: usize = w.dependents.values().map(|s| s.len()).sum();
                let rss = std::fs::read_to_string("/proc/self/statm")
                    .ok()
                    .and_then(|s| {
                        s.split_whitespace()
                            .nth(1)
                            .and_then(|v| v.parse::<u64>().ok())
                    })
                    .unwrap_or(0)
                    * 4;
                // RSS swings by tens of MB between identical runs (allocator, Mesa); glibc's
                // in-use heap is the stable figure for comparing builds.
                #[repr(C)]
                struct MallInfo2([usize; 10]);
                unsafe extern "C" {
                    fn mallinfo2() -> MallInfo2;
                }
                // SAFETY: glibc's mallinfo2(3) takes nothing and returns struct mallinfo2 (ten
                // size_t fields) by value, which MallInfo2 mirrors.
                let mi = unsafe { mallinfo2() };
                let heap = (mi.0[7] + mi.0[4]) / 1024;
                println!(
                    "panels {i}: items {} live {live} ctxs {} deps {}/{deps} assigned {} objs {} freed {freed} rss {rss}K heap {heap}K",
                    w.items.len(),
                    w.ctxs.len(),
                    w.dependents.len(),
                    w.assigned.len(),
                    qml::value::LIVE[1].load(std::sync::atomic::Ordering::Relaxed),
                );
            };
            sample(&mut rt, 0);
            for i in 1..=n {
                for f in &flags {
                    let t = Instant::now();
                    rt.set(gs, f, qml::value::Value::Bool(true));
                    drv.turn(&mut rt);
                    if i <= 2 {
                        println!("open {i} {f}: {:.1} ms", t.elapsed().as_secs_f64() * 1000.0);
                    }
                    drv.run(&mut rt, 300.0);
                    rt.set(gs, f, qml::value::Value::Bool(false));
                    drv.run(&mut rt, 300.0);
                }
                sample(&mut rt, i);
            }
        }
        s => panic!("unknown scenario {s}"),
    }
    let c1 = ticks();
    let b1 = qml::runtime::prof_counts();
    println!(
        "riscen {scen} N={n}: cpu {} ticks, wall {} ms, objects {}, bindings {}, handlers {}, errors {}",
        c1 - c0,
        t0.elapsed().as_millis(),
        rt.world.borrow().items.len() - o0,
        b1.0 - b0.0,
        b1.1 - b0.1,
        rt.world.borrow().errors.len()
    );
    if std::env::var_os("RISCEN_ERRORS").is_some() {
        for e in rt.world.borrow().errors.iter().take(40) {
            println!("error: {e}");
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
}
