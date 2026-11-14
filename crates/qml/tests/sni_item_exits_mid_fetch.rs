//! An item whose app exits while the tray's `GetAll` to it is unanswered is dropped: the bus
//! answers the call (`NoReply`) and the item's name loses its owner, which unregisters it. Its
//! fetch ends there, with nothing listed for it, and the tray goes on listing other items.

mod sni_watcher;

use qml::runtime::Runtime;
use sni_watcher::*;

#[test]
fn an_item_whose_app_exits_mid_fetch_is_dropped() {
    let Some(_bus) = bus(concat!(
        module_path!(),
        "::an_item_whose_app_exits_mid_fetch_is_dropped"
    )) else {
        return;
    };
    let mut rt = Runtime::new(None);
    let lp = Loop::new(&mut rt);
    let sigs = watcher_signals(lp.ping());
    let owners = watcher_owners(lp.ping());
    let root = rt.load_str(TRAY_QML).unwrap();
    lp.recv_until(&mut rt, &owners, "the tray serves the Watcher", |o| {
        !o.is_empty()
    });

    let (gone, _gate, entered) = held_item("gone", lp.ping());
    let key = format!("{}/StatusNotifierItem", gone.unique_name().unwrap());
    register(&gone);
    lp.recv_until(&mut rt, &entered, "the tray reading the held item", |_| {
        true
    });
    // The app exits with the read unanswered.
    gone.close().unwrap();
    lp.recv_until(&mut rt, &sigs, "the exited item unregistered", |s| {
        *s == Sig::ItemUnregistered(key.clone())
    });

    let other = item("other");
    register(&other);
    lp.until(&mut rt, "the next item listed", |rt| {
        ids(rt, root) == "other"
    });
    assert_eq!(ids(&rt, root), "other");
    let errs = rt.errors();
    assert!(errs.is_empty(), "{errs:?}");
}
