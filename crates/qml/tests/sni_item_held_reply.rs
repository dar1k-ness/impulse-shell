//! An item's `GetAll` reply has no deadline: an item the test answers only after the tray has
//! listed another one registered later is still listed, after it (the model appends an item
//! once it has loaded, as Quickshell's does). The hold ends when the test opens it, never on a
//! timer, so the tray's state cannot depend on how long it lasts; and the item that does not
//! answer holds up no other.

mod sni_watcher;

use qml::runtime::Runtime;
use sni_watcher::*;

#[test]
fn an_item_answering_late_is_listed_and_holds_up_no_other() {
    let Some(_bus) = bus(concat!(
        module_path!(),
        "::an_item_answering_late_is_listed_and_holds_up_no_other"
    )) else {
        return;
    };
    let mut rt = Runtime::new(None);
    let lp = Loop::new(&mut rt);
    let owners = watcher_owners(lp.ping());
    let root = rt.load_str(TRAY_QML).unwrap();
    lp.recv_until(&mut rt, &owners, "the tray serves the Watcher", |o| {
        !o.is_empty()
    });

    let (slow, gate, entered) = held_item("slow", lp.ping());
    register(&slow);
    lp.recv_until(&mut rt, &entered, "the tray reading the held item", |_| {
        true
    });
    // The marker: an item registered after the held one is fetched and listed meanwhile.
    let marker = item("marker");
    register(&marker);
    lp.until(&mut rt, "the marker item listed", |rt| {
        ids(rt, root) == "marker"
    });

    gate.open();
    // Listed once loaded: after the marker.
    lp.until(&mut rt, "the held item listed", |rt| {
        ids(rt, root) == "marker,slow"
    });
    let errs = rt.errors();
    assert!(errs.is_empty(), "{errs:?}");
}
