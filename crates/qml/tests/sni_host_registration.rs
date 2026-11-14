//! The Watcher's hosts per the StatusNotifierWatcher spec: `IsStatusNotifierHostRegistered`
//! holds while some registered host is running, a host is unregistered once its name loses
//! its owner (so one that restarts is announced again), and a tray gives its host name up
//! when it goes, though its process (and the Watcher on its connection) stays.

mod sni_watcher;

use qml::runtime::Runtime;
use sni_watcher::*;
use zbus::blocking::Connection;

const PREFIX: &str = "org.kde.StatusNotifierHost-";
const FOREIGN: &str = "org.kde.StatusNotifierHost-4242";

fn host_present(c: &Connection) -> bool {
    watcher_prop(c, "IsStatusNotifierHostRegistered")
        .try_into()
        .unwrap()
}

#[test]
fn hosts_are_registered_while_their_name_is_owned() {
    let Some(_bus) = bus(concat!(
        module_path!(),
        "::hosts_are_registered_while_their_name_is_owned"
    )) else {
        return;
    };
    let me = Connection::session().unwrap();
    let mut rt = Runtime::new(None);
    let lp = Loop::new(&mut rt);
    let sigs = watcher_signals(lp.ping());
    let names = name_owners(PREFIX, lp.ping());
    let _root = rt.load_str(TRAY_QML).unwrap();

    let (_, (tray_host, _, tray_conn)) =
        lp.recv_until(&mut rt, &names, "the tray's host name", |(_, _, new)| {
            !new.is_empty()
        });
    let (before, _) = lp.recv_until(&mut rt, &sigs, "the tray's host registers", |s| {
        *s == Sig::HostRegistered
    });
    assert!(before.is_empty(), "{before:?}");
    lp.recv_until(
        &mut rt,
        &sigs,
        "IsStatusNotifierHostRegistered turns true",
        |s| *s == Sig::HostPresent(true),
    );
    assert!(host_present(&me));

    let foreign = host(FOREIGN);
    lp.recv_until(&mut rt, &sigs, "the foreign host registers", |s| {
        *s == Sig::HostRegistered
    });
    drop(foreign);
    let (before, _) = lp.recv_until(&mut rt, &sigs, "the foreign host is unregistered", |s| {
        *s == Sig::HostUnregistered
    });
    assert!(before.is_empty(), "{before:?}");
    assert!(host_present(&me), "the tray's host still runs");

    let again = host(FOREIGN);
    lp.recv_until(
        &mut rt,
        &sigs,
        "the restarted foreign host registers again",
        |s| *s == Sig::HostRegistered,
    );
    drop(again);
    lp.recv_until(
        &mut rt,
        &sigs,
        "the restarted foreign host is unregistered",
        |s| *s == Sig::HostUnregistered,
    );

    drop(rt);
    recv_until(
        &names,
        "the tray gives its host name up",
        |(n, old, new)| *n == tray_host && *old == tray_conn && new.is_empty(),
    );
    let (before, _) = recv_until(&sigs, "the tray's host is unregistered", |s| {
        *s == Sig::HostUnregistered
    });
    assert!(before.is_empty(), "{before:?}");
    let (before, _) = recv_until(&sigs, "IsStatusNotifierHostRegistered turns false", |s| {
        *s == Sig::HostPresent(false)
    });
    assert!(before.is_empty(), "{before:?}");
    assert!(!host_present(&me));
}
