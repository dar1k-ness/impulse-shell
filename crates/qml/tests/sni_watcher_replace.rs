//! A foreign StatusNotifierWatcher is replaced by another: the tray registers its host with
//! the new one (Quickshell re-registers on `serviceRegistered`) and lists the new one's items.

mod sni_watcher;

use qml::runtime::Runtime;
use sni_watcher::*;

#[test]
fn tray_registers_with_a_watcher_that_replaces_the_foreign_one() {
    let Some(_bus) = bus(concat!(
        module_path!(),
        "::tray_registers_with_a_watcher_that_replaces_the_foreign_one"
    )) else {
        return;
    };
    let mut rt = Runtime::new(None);
    let lp = Loop::new(&mut rt);
    let (first, first_hosts) = foreign_watcher(vec![], false, lp.ping());

    let root = rt.load_str(TRAY_QML).unwrap();
    let mut host = String::new();
    lp.until(&mut rt, "the host registers with the first watcher", |_| {
        first_hosts.try_recv().map(|h| host = h).is_ok()
    });
    assert!(host.starts_with("org.kde.StatusNotifierHost-"), "{host}");

    let app = item("moved");
    let key = format!("{}/StatusNotifierItem", app.unique_name().unwrap());
    let (_second, second_hosts) = foreign_watcher(vec![key], true, lp.ping());
    lp.until(
        &mut rt,
        "the host registers with the second watcher",
        |_| second_hosts.try_recv().is_ok_and(|h| h == host),
    );
    lp.until(&mut rt, "the second watcher's item shows", |rt| {
        ids(rt, root) == "moved"
    });
    drop(first);
    let errs = rt.errors();
    assert!(errs.is_empty(), "{errs:?}");
}
