//! QQuickFrameAnimation: the cases `frame_animation_states.qml` and `frame_animation_hidden.qml`
//! of crates/rigen/check, which runs them with the interpreter and with their compiled code
//! against their `// qt:` line; here Qt 6.12 is asked again for that line. The number of steps a second, next to posted events
//! too, is crates/shell/tests/frame_animation_equiv.rs.
#![cfg(feature = "interp")]

#[path = "support/qt_log.rs"]
mod qt_log;

use std::path::PathBuf;

/// The `// qt:` line of a case of crates/rigen/check, from Qt 6.12 and from here.
fn as_qt(case: &str) {
    if !qt_log::qt_available() {
        return;
    }
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../rigen/check/cases/qml")
        .join(case);
    let src = std::fs::read_to_string(&file).unwrap();
    let line = src.lines().find_map(|l| l.strip_prefix("// qt: ")).unwrap();
    assert_eq!(qt_log::qt_result(&file), line, "Qt 6.12");
    assert_eq!(qt_log::our_result(&file), line);
}

/// The counters at each step, `pause()`, `resume()`, `reset()`, `restart()`, `stop()`,
/// `start()` and writes of `running` and `paused`, with the changed signals in their order.
#[test]
fn a_frame_animation_counts_and_reports_through_its_states_as_qt() {
    as_qt("frame_animation_states.qml");
}

/// In an item that is not visible, under a parent that is not, in a Window that is not, and
/// in an item hidden while it runs; and the order the jobs step in.
#[test]
fn a_frame_animation_is_triggered_where_nothing_is_shown_as_qt() {
    as_qt("frame_animation_hidden.qml");
}
