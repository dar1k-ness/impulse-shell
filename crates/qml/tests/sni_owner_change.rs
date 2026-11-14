//! An item registered by a well-known name that moves straight to another owner (a restarted
//! app taking it over with `ReplaceExisting`) shows the new owner's item: the registration
//! stays (no `ItemRegistered`), so the tray refetches it on the name's owner change.

mod sni_watcher;

use qml::runtime::Runtime;
use sni_watcher::*;
use zbus::fdo::{RequestNameFlags as F, RequestNameReply};

#[test]
fn a_name_taken_over_by_another_owner_shows_its_item() {
    let Some(_bus) = bus(concat!(
        module_path!(),
        "::a_name_taken_over_by_another_owner_shows_its_item"
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

    let name = format!("org.kde.StatusNotifierItem-{}-1", std::process::id());
    let first = item("first");
    let r = first
        .request_name_with_flags(name.as_str(), F::AllowReplacement | F::DoNotQueue)
        .unwrap();
    assert_eq!(r, RequestNameReply::PrimaryOwner);
    first
        .call_method(
            Some(WATCHER),
            "/StatusNotifierWatcher",
            Some(WATCHER),
            "RegisterStatusNotifierItem",
            &(name.as_str(),),
        )
        .unwrap();
    lp.until(&mut rt, "the first owner's item", |rt| {
        ids(rt, root) == "first"
    });

    let second = item("second");
    let r = second
        .request_name_with_flags(name.as_str(), F::ReplaceExisting | F::DoNotQueue)
        .unwrap();
    assert_eq!(r, RequestNameReply::PrimaryOwner);
    lp.until(&mut rt, "the new owner's item", |rt| {
        ids(rt, root) == "second"
    });
    drop(first);
    let errs = rt.errors();
    assert!(errs.is_empty(), "{errs:?}");
}
