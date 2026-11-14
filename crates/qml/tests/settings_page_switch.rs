//! settings.qml section switches: once the pages are preloaded, a switch builds nothing (each
//! page instantiation under switchAnim was a 30-80 ms frame) and only toggles which page shows.
//! The one item a switch may add is the `Crossfade` singleton, made the first time the window
//! asks for its fade.
use qml::runtime::Runtime;
use std::path::PathBuf;

fn ri() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

#[test]
fn settings_page_switch_builds_nothing() {
    const TEST: &str = concat!(module_path!(), "::settings_page_switch_builds_nothing");
    if !testenv::in_own_process(TEST) {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        std::fs::create_dir_all(home.join(".config/rusty-impulse")).unwrap();
        std::fs::write(home.join(".config/rusty-impulse/config.json"), "{}\n").unwrap();
        testenv::run_in_own_process(TEST, testenv::home_vars(home));
        return;
    }
    let r = ri();
    let mut rt = Runtime::new(Some(r.clone()));
    let dirs: Vec<PathBuf> = [
        "modules/common",
        "modules/common/functions",
        "modules/common/utils",
        "services",
        "",
    ]
    .iter()
    .map(|d| r.join(d))
    .collect();
    rt.load_singletons_lazy(&dirs);
    let t0 = std::time::Instant::now();
    let root = rt.load_file(&r.join("settings.qml")).unwrap();
    eprintln!("load {:?}", t0.elapsed());
    let mut now = 0f64;
    let mut pump = |rt: &mut Runtime, ms: u64| -> (f64, f64) {
        let (mut max, mut sum) = (0f64, 0f64);
        for _ in 0..ms / 8 {
            let f = std::time::Instant::now();
            rt.poll_native();
            now += 8.0;
            rt.tick(now);
            let d = f.elapsed().as_secs_f64() * 1000.0;
            max = max.max(d);
            sum += d;
        }
        (max, sum)
    };
    let alive = |rt: &Runtime| {
        let w = rt.world.borrow();
        w.items.len() - w.dead.len() - usize::from(w.singletons.contains_key("Crossfade"))
    };
    eprintln!("settle {:?}", pump(&mut rt, 1500));
    let pages_v = rt.world.borrow_mut().read(root, "pages");
    let pages = match pages_v {
        Some(qml::value::Value::Obj(o)) => o.borrow().array.as_ref().map_or(0, |a| a.len()),
        _ => 0,
    };
    assert!(pages > 1, "settings.qml lists no pages");
    let before = alive(&rt);
    for p in (1..pages).chain(0..pages) {
        let p = p as f64;
        rt.world
            .borrow_mut()
            .write(root, "currentPage", qml::value::Value::Num(p));
        let f = std::time::Instant::now();
        rt.settle();
        let s = f.elapsed().as_secs_f64() * 1000.0;
        let (max, sum) = pump(&mut rt, 600);
        let n = alive(&rt);
        eprintln!("page {p}: settle {s:.1}ms max frame {max:.1}ms total {sum:.1}ms items {n}");
        assert_eq!(n, before, "switching to page {p} built or dropped items");
    }
    let errs = rt
        .world
        .borrow_mut()
        .errors
        .drain(..)
        .take(10)
        .collect::<Vec<_>>();
    assert!(errs.is_empty(), "{errs:?}");
}
