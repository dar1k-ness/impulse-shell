//! Starting the tray, notification server and polkit agent, and sending a notification with
//! `Notify.send`, never wait on the bus on the main thread: with session and system buses that
//! accept connections but never answer, the main loop keeps running.

use qml::runtime::Runtime;
use std::os::unix::net::UnixListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

const QML: &str = "import QtQuick\nimport Rustshell.Services.SystemTray\n\
    import Rustshell.Services.Notifications\nimport Rustshell.Services.Polkit\n\
    Item { property int n: SystemTray.items.values.length\n NotificationServer {}\n PolkitAgent {} }";

/// Both buses (one socket here) accept connections and never answer. The services dial both
/// while the main loop keeps turning: a turn that waited on the bus would never return, and
/// nextest's slow-timeout ends the test.
#[test]
fn service_startup_does_not_block_on_a_silent_bus() {
    const TEST: &str = concat!(
        module_path!(),
        "::service_startup_does_not_block_on_a_silent_bus"
    );
    // The services find both buses through the environment: run in a process of its own with
    // both addresses naming a socket in a fresh directory.
    if !testenv::in_own_process(TEST) {
        let dir = tempfile::tempdir().unwrap();
        let addr = format!("unix:path={}", dir.path().join("bus").display());
        testenv::run_in_own_process(
            TEST,
            [
                ("DBUS_SESSION_BUS_ADDRESS", &addr),
                ("DBUS_SYSTEM_BUS_ADDRESS", &addr),
            ],
        );
        return;
    }
    let addr = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    let sock = addr.strip_prefix("unix:path=").unwrap();
    let l = UnixListener::bind(sock).unwrap();
    let mut rt = Runtime::new(None);
    let wake = testenv::Wakeups::new();
    rt.world.borrow().waker.set_ping(wake.ping());
    let dialed = Arc::new(AtomicUsize::new(0));
    {
        let (dialed, ping) = (dialed.clone(), wake.ping());
        std::thread::spawn(move || {
            let mut held = vec![];
            for s in l.incoming().flatten() {
                held.push(s);
                dialed.fetch_add(1, Ordering::SeqCst);
                ping();
            }
        });
    }
    rt.load_str(QML).unwrap();
    rt.settle();
    // The session bus (tray, notifications) and the system bus (polkit), one shared
    // connection each.
    eprintln!("waiting for both buses to be dialed");
    loop {
        rt.poll_native();
        rt.settle();
        if dialed.load(Ordering::SeqCst) >= 2 {
            break;
        }
        wake.wait(None, None);
    }
    // And the loop turns on while neither answers.
    rt.poll_native();
    rt.settle();
}

/// A runtime on session and system buses (one socket) that accept connections and never
/// answer, in a process of its own; `None` in the process that only started that one. `dialed`
/// counts the connections accepted so far, and the wakeups are pinged at each.
fn on_a_silent_bus(test: &str) -> Option<(Runtime, testenv::Wakeups, Arc<AtomicUsize>)> {
    if !testenv::in_own_process(test) {
        let dir = tempfile::tempdir().unwrap();
        let addr = format!("unix:path={}", dir.path().join("bus").display());
        testenv::run_in_own_process(
            test,
            [
                ("DBUS_SESSION_BUS_ADDRESS", &addr),
                ("DBUS_SYSTEM_BUS_ADDRESS", &addr),
            ],
        );
        return None;
    }
    let addr = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    let l = UnixListener::bind(addr.strip_prefix("unix:path=").unwrap()).unwrap();
    let rt = Runtime::new(None);
    let wake = testenv::Wakeups::new();
    rt.world.borrow().waker.set_ping(wake.ping());
    let dialed = Arc::new(AtomicUsize::new(0));
    {
        let (dialed, ping) = (dialed.clone(), wake.ping());
        std::thread::spawn(move || {
            let mut held = vec![];
            for s in l.incoming().flatten() {
                held.push(s);
                dialed.fetch_add(1, Ordering::SeqCst);
                ping();
            }
        });
    }
    Some((rt, wake, dialed))
}

/// Turns the loop until the silent bus has accepted a connection.
fn wait_dialed(rt: &mut Runtime, wake: &testenv::Wakeups, dialed: &AtomicUsize) {
    eprintln!("waiting for the session bus to be dialed");
    loop {
        rt.poll_native();
        rt.settle();
        if dialed.load(Ordering::SeqCst) >= 1 {
            return;
        }
        wake.wait(None, None);
    }
}

/// `Notify.send` without buttons is the first to need the session bus, which accepts the
/// connection and never answers: the send returns, a thread dials, and the loop turns on. A
/// send that dialed on the main thread would never return, and nextest's slow-timeout ends
/// the test.
#[test]
fn notify_send_does_not_dial_a_silent_bus_on_the_main_thread() {
    const TEST: &str = concat!(
        module_path!(),
        "::notify_send_does_not_dial_a_silent_bus_on_the_main_thread"
    );
    let Some((mut rt, wake, dialed)) = on_a_silent_bus(TEST) else {
        return;
    };
    rt.load_str(
        "import QtQuick\nimport Impulse\n\
         Item { Component.onCompleted: Notify.send(\"Weather\", \"Refreshing\") }",
    )
    .unwrap();
    rt.settle();
    wait_dialed(&mut rt, &wake, &dialed);
    rt.poll_native();
    rt.settle();
}

/// The notification server is dialing the session bus, which never answers, when `Notify.send`
/// is called: the send does not wait for that dial to end either.
#[test]
fn notify_send_does_not_wait_for_a_dial_of_a_silent_bus() {
    const TEST: &str = concat!(
        module_path!(),
        "::notify_send_does_not_wait_for_a_dial_of_a_silent_bus"
    );
    let Some((mut rt, wake, dialed)) = on_a_silent_bus(TEST) else {
        return;
    };
    let root = rt
        .load_str(
            "import QtQuick\nimport Impulse\nimport Rustshell.Services.Notifications\n\
             Item { NotificationServer {}\n\
             function send() { return Notify.send(\"Weather\", \"Refreshing\") } }",
        )
        .unwrap();
    rt.settle();
    wait_dialed(&mut rt, &wake, &dialed);
    let id = rt.call_method(root, "send", vec![]).unwrap().unwrap();
    assert_eq!(id.to_number(), 1.0);
    rt.poll_native();
    rt.settle();
}

/// The shell's exit waits for the sends still queued (`notifier::flush`), and for a session bus
/// that never answers only as long as its limit: the process then ends without them. A flush
/// without the limit would never return, and nextest's slow-timeout ends the test.
#[test]
fn notify_flush_gives_up_on_a_silent_bus_at_its_limit() {
    const TEST: &str = concat!(
        module_path!(),
        "::notify_flush_gives_up_on_a_silent_bus_at_its_limit"
    );
    let Some((mut rt, wake, dialed)) = on_a_silent_bus(TEST) else {
        return;
    };
    rt.load_str(
        "import QtQuick\nimport Impulse\n\
         Item { Component.onCompleted: Notify.send(\"Weather\", \"Refreshing\") }",
    )
    .unwrap();
    rt.settle();
    wait_dialed(&mut rt, &wake, &dialed);
    assert!(
        !qml::notifier::flush(),
        "the send is still queued: only the limit ends the wait"
    );
}

const PLAIN: &str = "Notify.send(\"Weather\", \"Refreshing\")";
const BUTTONS: &str = "Notify.send(\"Weather\", \"\", { actions: [\"Open\", \"Later\"] })";

/// The system refuses the main thread new threads (the limit on threads is reached), so the
/// queue `send` goes through gets none, on a session bus that never answers. The send returns
/// with its id and one line in the log, and its caller has not dialed the bus: the bus has
/// answered nothing when it returns. `dialing`: the notification server is dialing the bus
/// meanwhile, and holds the connection's slot until the bus answers; else nothing in the
/// process has used the bus yet. A send that went to the bus on its caller would never return
/// behind the dial, or would dial, and nextest's slow-timeout ends the test.
fn a_send_refused_its_thread_returns_on_a_silent_bus(test: &str, send: &str, dialing: bool) {
    let Some((mut rt, wake, dialed)) = on_a_silent_bus(test) else {
        return;
    };
    let (import, server) = match dialing {
        true => (
            "import Rustshell.Services.Notifications\n",
            "NotificationServer {}\n",
        ),
        false => ("", ""),
    };
    let root = rt
        .load_str(&format!(
            "import QtQuick\nimport Impulse\n{import}\
             Item {{ {server}function send() {{ return {send} }} }}"
        ))
        .unwrap();
    rt.settle();
    if dialing {
        wait_dialed(&mut rt, &wake, &dialed);
    }
    let dials = dialed.load(Ordering::SeqCst);
    assert_eq!(dials, dialing as usize);
    testenv::refuse_new_threads();
    let mut id = None;
    let log = testenv::stderr_of(|| id = rt.call_method(root, "send", vec![]).unwrap().ok());
    assert_eq!(id.map(|v| v.to_number()), Some(1.0));
    let lines: Vec<_> = log.lines().collect();
    assert_eq!(lines.len(), 1, "{log}");
    assert!(
        lines[0].starts_with("impulse-shell: cannot start a lane thread: "),
        "{log}"
    );
    assert_eq!(
        dialed.load(Ordering::SeqCst),
        dials,
        "the send dialed the bus"
    );
    rt.poll_native();
    rt.settle();
}

#[test]
fn a_send_refused_its_thread_does_not_wait_for_a_dial_of_a_silent_bus() {
    const TEST: &str = concat!(
        module_path!(),
        "::a_send_refused_its_thread_does_not_wait_for_a_dial_of_a_silent_bus"
    );
    a_send_refused_its_thread_returns_on_a_silent_bus(TEST, PLAIN, true);
}

#[test]
fn a_send_with_buttons_refused_its_thread_does_not_wait_for_a_dial_of_a_silent_bus() {
    const TEST: &str = concat!(
        module_path!(),
        "::a_send_with_buttons_refused_its_thread_does_not_wait_for_a_dial_of_a_silent_bus"
    );
    a_send_refused_its_thread_returns_on_a_silent_bus(TEST, BUTTONS, true);
}

#[test]
fn a_send_refused_its_thread_does_not_dial_a_silent_bus() {
    const TEST: &str = concat!(
        module_path!(),
        "::a_send_refused_its_thread_does_not_dial_a_silent_bus"
    );
    a_send_refused_its_thread_returns_on_a_silent_bus(TEST, PLAIN, false);
}

#[test]
fn a_send_with_buttons_refused_its_thread_does_not_dial_a_silent_bus() {
    const TEST: &str = concat!(
        module_path!(),
        "::a_send_with_buttons_refused_its_thread_does_not_dial_a_silent_bus"
    );
    a_send_refused_its_thread_returns_on_a_silent_bus(TEST, BUTTONS, false);
}
