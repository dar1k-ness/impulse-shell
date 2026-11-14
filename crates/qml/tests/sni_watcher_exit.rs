//! A foreign StatusNotifierWatcher owns the name when the tray starts, then exits: the tray
//! claims the name and serves its own Watcher (Quickshell retries on `serviceUnregistered`),
//! so apps registering afterwards show up.

mod sni_watcher;

use qml::runtime::Runtime;
use sni_watcher::*;

#[test]
fn tray_claims_the_watcher_name_once_a_foreign_watcher_exits() {
    let Some(_bus) = bus(concat!(
        module_path!(),
        "::tray_claims_the_watcher_name_once_a_foreign_watcher_exits"
    )) else {
        return;
    };
    let mut rt = Runtime::new(None);
    let lp = Loop::new(&mut rt);
    let owners = watcher_owners(lp.ping());
    let (foreign, hosts) = foreign_watcher(vec![], false, lp.ping());
    let foreign_name = foreign.unique_name().unwrap().to_string();
    lp.until(&mut rt, "the foreign watcher owns the name", |_| {
        owners.try_recv().is_ok_and(|o| o == foreign_name)
    });

    let root = rt.load_str(TRAY_QML).unwrap();
    lp.until(
        &mut rt,
        "the host registers with the foreign watcher",
        |_| hosts.try_recv().is_ok(),
    );

    drop(foreign);
    let mut owner = None;
    lp.until(&mut rt, "the Watcher name gets an owner again", |_| {
        owner = owners.try_iter().filter(|o| !o.is_empty()).last();
        owner.is_some()
    });
    assert_ne!(owner.as_deref(), Some(foreign_name.as_str()));

    // The tray's own Watcher now takes registrations.
    let app = item("late");
    app.call_method(
        Some(WATCHER),
        "/StatusNotifierWatcher",
        Some(WATCHER),
        "RegisterStatusNotifierItem",
        &(app.unique_name().unwrap().as_str(),),
    )
    .unwrap();
    lp.until(&mut rt, "the late item shows", |rt| ids(rt, root) == "late");
    let errs = rt.errors();
    assert!(errs.is_empty(), "{errs:?}");
}
