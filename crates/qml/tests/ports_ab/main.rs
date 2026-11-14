//! QML-level A/B tests of `crates/qml/src/ports`: each runs a reference written with real bash
//! and tools (`RI_AB_REFERENCE=1`) and the `Impulse` types on the same inputs and compares effects.

mod ab;
mod clip;
#[path = "../runtime_host/mod.rs"]
mod host;
mod shot;

/// The child process of every A/B side (`ab::side`); a no-op unless `RI_AB_ROOT` is set.
#[test]
#[ignore = "child process: `ab::side` runs it for each side of an A/B test"]
fn ab_child() {
    ab::child_main();
}
