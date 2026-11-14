//! When a change handler runs, against Qt 6.12: inside the write, before the bindings that
//! read the property are evaluated again. The test steps the probe of `handler_order/` on a
//! 16 ms clock, calling its `sample()` at the end of every frame, and expects the lines
//! Qt 6.12 printed for it on the same clock (`<probe>.qt612.txt`, written by a
//! `QAnimationDriver` harness).

use qml::runtime::Runtime;

/// The probe's `log` after 10 frames.
fn run(src: &str) -> String {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(src).unwrap();
    rt.settle();
    rt.run_later();
    for i in 1..=10 {
        rt.tick(i as f64 * 16.0);
        rt.call_method(root, "sample", Vec::new()).unwrap().unwrap();
        rt.settle();
    }
    rt.get(root, "log").to_js_string()
}

/// `on<Property>Changed` sees the bindings that read the property as they were before the
/// change: after a write from JavaScript, after a binding's new value and after an
/// animation's step. Each of those bindings is evaluated after it, followed by its own
/// handler (`Hyprsunset.qml`: `onConfigReadyChanged` reads `automatic`, bound to
/// `configReady`).
/// `log` of the probe `src` is what Qt 6.12 printed, line for line.
fn same(src: &str, qt: &str) {
    let log = run(src);
    let (ours, qt): (Vec<&str>, Vec<&str>) = (log.lines().collect(), qt.lines().collect());
    for (i, (a, b)) in ours.iter().zip(qt.iter()).enumerate() {
        assert_eq!(a, b, "line {} (ours, then Qt 6.12)", i + 1);
    }
    assert_eq!(ours.len(), qt.len(), "number of lines");
}

#[test]
fn handler_runs_before_the_bindings_that_read_the_property() {
    same(
        include_str!("handler_order/ready.qml"),
        include_str!("handler_order/ready.qt612.txt"),
    );
}

/// The shape of `qml/services/Hyprsunset.qml`: `onConfigReadyChanged` finds `automatic` as
/// it was, so the temperature is reset before the schedule sets it.
#[test]
fn handler_of_the_config_sees_the_schedule_off() {
    same(
        include_str!("handler_order/sunset.qml"),
        include_str!("handler_order/sunset.qt612.txt"),
    );
}

/// Handlers that are not the object's own are called in the order Qt connected them: a
/// handler declared on another object that reads the property through a binding of its own
/// and a `Connections` come after the bindings made before them.
#[test]
fn handlers_and_bindings_run_in_the_order_they_were_connected() {
    same(
        include_str!("handler_order/conn.qml"),
        include_str!("handler_order/conn.qt612.txt"),
    );
}
