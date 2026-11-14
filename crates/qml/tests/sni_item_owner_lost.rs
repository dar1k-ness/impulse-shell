//! An item whose bus name loses its owner leaves the Watcher's `RegisteredStatusNotifierItems`
//! with `StatusNotifierItemUnregistered` and a `PropertiesChanged` for the property, as its
//! registration added it with `StatusNotifierItemRegistered` and one.

mod sni_watcher;

use qml::runtime::Runtime;
use sni_watcher::*;
use zbus::blocking::Connection;

/// The item (not host) signals of `sigs`.
fn item_sigs(sigs: Vec<Sig>) -> Vec<Sig> {
    sigs.into_iter()
        .filter(|s| {
            matches!(
                s,
                Sig::ItemRegistered(_) | Sig::ItemUnregistered(_) | Sig::Items(_)
            )
        })
        .collect()
}

#[test]
fn an_item_whose_owner_goes_is_unlisted_with_a_property_change() {
    let Some(_bus) = bus(concat!(
        module_path!(),
        "::an_item_whose_owner_goes_is_unlisted_with_a_property_change"
    )) else {
        return;
    };
    let me = Connection::session().unwrap();
    let mut rt = Runtime::new(None);
    let lp = Loop::new(&mut rt);
    let sigs = watcher_signals(lp.ping());
    let owners = watcher_owners(lp.ping());
    let _root = rt.load_str(TRAY_QML).unwrap();
    lp.recv_until(&mut rt, &owners, "the tray serves the Watcher", |o| {
        !o.is_empty()
    });

    let app = item("dropped");
    let key = format!("{}/StatusNotifierItem", app.unique_name().unwrap());
    app.call_method(
        Some(WATCHER),
        "/StatusNotifierWatcher",
        Some(WATCHER),
        "RegisterStatusNotifierItem",
        &(app.unique_name().unwrap().as_str(),),
    )
    .unwrap();
    let (before, _) = lp.recv_until(&mut rt, &sigs, "the registration's property change", |s| {
        *s == Sig::Items(vec![key.clone()])
    });
    assert_eq!(item_sigs(before), [Sig::ItemRegistered(key.clone())]);

    drop(app);
    let (before, change) = lp.recv_until(
        &mut rt,
        &sigs,
        "a property change once the item's owner is gone",
        |s| matches!(s, Sig::Items(_)),
    );
    assert_eq!(item_sigs(before), [Sig::ItemUnregistered(key)]);
    assert_eq!(change, Sig::Items(vec![]));
    let listed: Vec<String> = watcher_prop(&me, "RegisteredStatusNotifierItems")
        .try_into()
        .unwrap();
    assert!(listed.is_empty(), "{listed:?}");
}
