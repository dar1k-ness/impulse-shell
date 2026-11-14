//! The World of the whole shell ends with its Runtime. Nothing the tree leaves behind holds
//! it: not an interpreter parked after a synchronous update (`World::spare`), a callback, a
//! service or a job. Runs the tree the build compiled (`qml/shell.qml` with its panel
//! families), headless, in a home of its own.

use qml::runtime::Runtime;
use qml::value::Value;
use std::path::PathBuf;
use std::rc::Rc;

const TEST: &str = concat!(
    module_path!(),
    "::the_world_of_the_whole_shell_ends_with_its_runtime"
);

/// Whether the right sidebar's window is shown.
fn sidebar_shown(rt: &Runtime) -> bool {
    let wins = shell::scene::find_windows(rt, "rustshell:sidebarRight");
    assert_eq!(wins.len(), 1, "sidebarRight windows");
    rt.get(wins[0], "visible").truthy()
}

#[test]
fn the_world_of_the_whole_shell_ends_with_its_runtime() {
    if !testenv::in_own_process(TEST) {
        let home = tempfile::tempdir().unwrap();
        testenv::run_in_own_process(TEST, testenv::home_vars(home.path()));
        return;
    }
    let root = PathBuf::from(ricompiled::ROOT);
    let mut rt = Runtime::new(Some(root.clone()));
    rt.eng()
        .set_compiled(Some(Rc::new(ricompiled::Compiled::new(&root))));
    let mut dirs = vec![
        root.join("modules/common"),
        root.join("modules/common/functions"),
        root.join("modules/common/utils"),
        root.join("services"),
        root.clone(),
    ];
    for family in ["modules/rusty", "modules/waffle"] {
        let mut d: Vec<PathBuf> = std::fs::read_dir(root.join(family))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.is_dir())
            .collect();
        d.sort();
        dirs.extend(d);
    }
    rt.load_singletons(&dirs);
    rt.set_screens(&[("Virtual-1".into(), 0, 0, 1920, 1080)]);
    rt.load_file(&root.join("shell.qml")).expect("shell.qml");
    let mut drv = qml::headless::Driver::new(true);
    drv.run(&mut rt, 4000.0);

    let states = *rt
        .world
        .borrow()
        .singletons
        .get("GlobalStates")
        .expect("GlobalStates");
    assert!(!sidebar_shown(&rt), "the sidebar starts closed");
    rt.set(states, "sidebarRightOpen", Value::Bool(true));
    drv.run(&mut rt, 2000.0);
    assert!(sidebar_shown(&rt), "the sidebar opened");
    rt.set(states, "sidebarRightOpen", Value::Bool(false));
    drv.run(&mut rt, 2000.0);
    assert!(!sidebar_shown(&rt), "the sidebar closed");

    let stop = rt.world.borrow().jobs_stop.0.clone();
    let world = Rc::downgrade(&rt.world);
    drop(drv);
    drop(rt);
    assert_eq!(
        world.strong_count(),
        0,
        "the World outlived its Runtime: something still holds it"
    );
    assert!(stop.load(std::sync::atomic::Ordering::Acquire));
}
