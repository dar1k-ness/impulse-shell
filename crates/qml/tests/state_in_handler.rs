//! A `when` that turns true while its group applies a state (QQuickStateGroupPrivate::
//! setCurrentStateInternal, `applyingState`): Qt 6.12 drops the change. The group stays in the
//! state it was applying, also when the `when` stays true, until a `when` changes again. Each
//! test steps a probe from `state_in_handler/` on a 16 ms clock, calling its `sample()` at the
//! end of every frame, and expects the lines Qt 6.12 gave for it (`<probe>.qt612.txt`, written
//! by `state_in_handler/qt.py`).

use qml::runtime::Runtime;

/// The probe's `log` after 60 frames.
fn run(src: &str) -> String {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(src).unwrap();
    rt.settle();
    rt.run_later();
    for i in 1..=60 {
        rt.tick(i as f64 * 16.0);
        rt.call_method(root, "sample", Vec::new()).unwrap().unwrap();
        rt.settle();
    }
    rt.get(root, "log").to_js_string()
}

/// Qt's lines of a `.qt612.txt`, without its warnings.
fn qt_said(qt: &str) -> Vec<&str> {
    (qt.lines().filter(|l| !l.starts_with("warning: "))).collect()
}

fn same_as_qt(src: &str, qt: &str) {
    let log = run(src);
    assert_eq!(
        log.lines().collect::<Vec<_>>(),
        qt_said(qt),
        "ours, then Qt 6.12"
    );
}

macro_rules! probe {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[test]
        fn $name() {
            same_as_qt(
                include_str!(concat!("state_in_handler/", stringify!($name), ".qml")),
                include_str!(concat!("state_in_handler/", stringify!($name), ".qt612.txt")),
            );
        }
    };
}

probe!(
    /// B's `when` turns true in the handler of the change to A and false again while the
    /// Transition to A runs: the group never enters B.
    when_back
);
probe!(
    /// The same with the `when` left true: still A, until another `when` changes.
    when_stays
);
probe!(
    /// True and false again inside the handler.
    when_twice
);
probe!(
    /// Without a Transition, in the handler of a property the state writes.
    when_in_write
);
probe!(
    /// In a handler the Transition's animation runs the change to A is over, so B is entered
    /// there and then.
    when_in_transition
);
