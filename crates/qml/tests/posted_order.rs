//! The order of posted events: `Qt.callLater`, a Timer with interval 0, the document of an
//! asynchronous component, the incubator. The fixtures are the cases `posted_order_*.qml` of
//! crates/rigen/check, which runs them with the interpreter and with their compiled code
//! against their `// qt:` line; here Qt 6.12 is asked again for that line.
#![cfg(feature = "interp")]

#[path = "support/qt_log.rs"]
mod qt_log;

use qt_log::{our_result, qt_result};
use std::path::PathBuf;

/// The `// qt:` line of a case of crates/rigen/check is what Qt 6.12 answers, and what the
/// interpreter answers. The line is a list of cases with `between` between them.
#[track_caller]
fn pinned(case: &str, between: &str) {
    if !qt_log::qt_available() {
        return;
    }
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../rigen/check/cases/qml")
        .join(format!("{case}.qml"));
    let src = std::fs::read_to_string(&file).unwrap();
    let line = src.lines().find_map(|l| l.strip_prefix("// qt: ")).unwrap();
    // The document of a component comes from Qt's loader thread and its incubator from a
    // timer of the window: on a loaded machine a run may see the document there at once
    // (`S!1`) or the incubator late in one case or another. Each case has to come out as
    // pinned in one of the runs.
    let want: Vec<&str> = line.split(between).collect();
    let mut seen = vec![false; want.len()];
    let mut last = String::new();
    for _ in 0..8 {
        last = qt_result(&file);
        let got: Vec<&str> = last.split(between).collect();
        assert_eq!(got.len(), want.len(), "{case}: Qt 6.12 says {last}");
        for (s, (g, w)) in seen.iter_mut().zip(got.iter().zip(&want)) {
            *s |= g == w;
        }
        if seen.iter().all(|s| *s) {
            break;
        }
    }
    let never: Vec<&str> = (want.iter().zip(&seen))
        .filter_map(|(w, s)| (!s).then_some(*w))
        .collect();
    assert!(
        never.is_empty(),
        "{case}: Qt 6.12 never said {never:?}; its last run: {last}"
    );
    assert_eq!(our_result(&file), line, "{case}");
}

/// The 24 orders of asking for a call, a Timer with interval 0, a component and an incubator.
#[test]
fn a_call_and_a_timer_of_interval_0_run_in_the_order_asked() {
    pinned("posted_order_permutations", " ");
}

/// `stop()`, `restart()`, a binding of `running`, a change of `interval`, posting from a
/// posted event, a chain of Timers against the clock.
#[test]
fn a_posted_trigger_keeps_its_place_and_what_a_turn_posts_goes_to_the_tail() {
    pinned("posted_order_cases", ";");
}

/// Timers declared running post when their component is complete, before
/// `Component.onCompleted`.
#[test]
fn a_timer_declared_running_posts_when_its_component_is_complete() {
    pinned("posted_order_declared", ";");
}

/// An order not kept on purpose (docs/qt-differences.md): a handler that stays busy after
/// asking for a component.
#[test]
fn the_document_of_a_component_arrives_after_the_handler_that_asked() {
    if !qt_log::qt_available() {
        return;
    }
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/posted_order/Busy.qml");
    assert_eq!(qt_result(&file), "S C T");
    assert_eq!(our_result(&file), "C T S");
}

/// An order not kept yet: a Loader declared before running Timers loads before they are
/// complete, where Qt completes it after them.
#[test]
fn a_loader_declared_before_running_timers_loads_first_here() {
    if !qt_log::qt_available() {
        return;
    }
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/posted_order/LoaderFirst.qml");
    assert_eq!(qt_result(&file), "P O B A D c0 c2 c1 L P");
    assert_eq!(our_result(&file), "D c0 c2 c1 P O B A L P");
}
