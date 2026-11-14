//! The StatusNotifierWatcher belongs to the process's shared session connection, not to one
//! tray: a tray started after another was dropped (same process, so the connection keeps the
//! `org.kde.StatusNotifierWatcher` name and no item sees it change owner, which is when the
//! SNI spec has items register again) lists the items registered before it, minus those whose
//! name lost its owner in between.

mod sni_watcher;

use qml::runtime::Runtime;
use sni_watcher::{Loop, bus, item, watcher_owners};
use zbus::blocking::Connection;

/// Item `id`, registered with the Watcher.
fn registered_item(id: &'static str) -> Connection {
    let c = item(id);
    c.call_method(
        Some(sni_watcher::WATCHER),
        "/StatusNotifierWatcher",
        Some(sni_watcher::WATCHER),
        "RegisterStatusNotifierItem",
        &(c.unique_name().unwrap().as_str(),),
    )
    .unwrap();
    c
}

fn registered(c: &Connection) -> Vec<String> {
    let p = zbus::blocking::fdo::PropertiesProxy::builder(c)
        .destination(sni_watcher::WATCHER)
        .unwrap()
        .path("/StatusNotifierWatcher")
        .unwrap()
        .build()
        .unwrap();
    let v = p
        .get(
            sni_watcher::WATCHER.try_into().unwrap(),
            "RegisteredStatusNotifierItems",
        )
        .unwrap();
    Vec::<String>::try_from(v).unwrap()
}

/// The Watcher's `StatusNotifierItemUnregistered` signals, from a match rule added before this
/// returns.
fn unregistrations(c: &Connection) -> zbus::blocking::MessageIterator {
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(sni_watcher::WATCHER)
        .unwrap()
        .path("/StatusNotifierWatcher")
        .unwrap()
        .member("StatusNotifierItemUnregistered")
        .unwrap()
        .build();
    zbus::blocking::MessageIterator::for_match_rule(rule, c, None).unwrap()
}

const TRAY_QML: &str = "import QtQuick\nimport Rustshell.Services.SystemTray\nItem {\n\
    readonly property string ids: SystemTray.items.values.map(i => i.id).sort().join(',') }";

fn ids(rt: &Runtime, root: usize) -> String {
    rt.get(root, "ids").to_js_string()
}

#[test]
fn a_later_tray_lists_the_items_registered_before_it() {
    let Some(_bus) = bus(concat!(
        module_path!(),
        "::a_later_tray_lists_the_items_registered_before_it"
    )) else {
        return;
    };
    let me = Connection::session().unwrap();
    let mut rt = Runtime::new(None);
    let lp = Loop::new(&mut rt);
    let owners = watcher_owners(lp.ping());
    let root = rt.load_str(TRAY_QML).unwrap();
    lp.until(&mut rt, "the tray to serve the Watcher", |_| {
        owners.try_iter().any(|o| !o.is_empty())
    });
    let keep = registered_item("keep");
    let gone = registered_item("gone");
    lp.until(&mut rt, "the first tray to list both items", |rt| {
        ids(rt, root) == "gone,keep"
    });
    drop(lp);
    drop(rt);

    // Between the trays: an item's name loses its owner, and the Watcher (on the shared
    // connection, which outlives the tray) drops its registration.
    let gone_key = format!("{}/StatusNotifierItem", gone.unique_name().unwrap());
    let unregistered = unregistrations(&me);
    drop(gone);
    eprintln!("waiting for the Watcher to unregister {gone_key}");
    assert!(
        unregistered
            .filter_map(|m| m.ok()?.body().deserialize::<String>().ok())
            .any(|k| k == gone_key)
    );

    let mut rt = Runtime::new(None);
    let lp = Loop::new(&mut rt);
    let root = rt.load_str(TRAY_QML).unwrap();
    lp.until(
        &mut rt,
        "the second tray to list only the kept item",
        |rt| ids(rt, root) == "keep",
    );
    let keep_key = format!("{}/StatusNotifierItem", keep.unique_name().unwrap());
    assert_eq!(registered(&me), [keep_key]);
}
