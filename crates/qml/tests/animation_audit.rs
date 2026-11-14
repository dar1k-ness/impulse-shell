//! Animation findings of the second QML engine audit against Qt 6.12 (20, 24, 34, 35). Each
//! test steps a probe from `animation_audit/` on a 16 ms clock, calling its `sample()` at the
//! end of every frame, and expects the lines Qt 6.12 printed for it on the same clock
//! (`<probe>.qt612.txt`, written by a `QAnimationDriver` harness).

use qml::runtime::Runtime;

/// The probe's `log` after 60 frames, and the warnings the engine gave.
fn run(src: &str) -> (String, Vec<String>) {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(src).unwrap();
    rt.settle();
    rt.run_later();
    for i in 1..=60 {
        rt.tick(i as f64 * 16.0);
        rt.call_method(root, "sample", Vec::new()).unwrap().unwrap();
        rt.settle();
    }
    (rt.get(root, "log").to_js_string(), rt.errors())
}

fn same_as_qt(src: &str, qt: &str) {
    let (log, _) = run(src);
    let (ours, qt): (Vec<&str>, Vec<&str>) = (log.lines().collect(), qt.lines().collect());
    for (i, (a, b)) in ours.iter().zip(qt.iter()).enumerate() {
        assert_eq!(a, b, "line {} (ours, then Qt 6.12)", i + 1);
    }
    assert_eq!(ours.len(), qt.len(), "number of lines");
}

/// Finding 20: `RotationAnimation.direction` picks the way round (Numerical, Shortest,
/// Clockwise, Counterclockwise), also for `rotation` and `angle` in a Transition.
#[test]
fn rotation_direction() {
    same_as_qt(
        include_str!("animation_audit/rot.qml"),
        include_str!("animation_audit/rot.qt612.txt"),
    );
}

/// Finding 35: a `ColorAnimation` without `property` in a Transition animates the changed
/// properties declared `color` (grouped ones too), and nothing else.
#[test]
fn color_animation_without_property() {
    same_as_qt(
        include_str!("animation_audit/trans.qml"),
        include_str!("animation_audit/trans.qt612.txt"),
    );
}

/// Finding 35: the same with `target` or `exclude` naming the objects.
#[test]
fn color_animation_target_and_exclude() {
    same_as_qt(
        include_str!("animation_audit/exclude.qml"),
        include_str!("animation_audit/exclude.qt612.txt"),
    );
}

/// Finding 35: one with `to` goes there, and the Transition's end writes the state's values.
#[test]
fn color_animation_to_then_state_values() {
    same_as_qt(
        include_str!("animation_audit/excludeto.qml"),
        include_str!("animation_audit/excludeto.qt612.txt"),
    );
}

/// Finding 35: the state's values come as they were written (a string stays a string in a
/// `var`, is a colour in `border.color`), also after a NumberAnimation with its own `to`.
#[test]
fn transition_end_writes_state_values() {
    same_as_qt(
        include_str!("animation_audit/statevalues.qml"),
        include_str!("animation_audit/statevalues.qt612.txt"),
    );
}

/// Finding 24: `SmoothedAnimation` speeds up and slows down (`velocity`, `duration`,
/// `maximumEasingTime`), keeps its speed when retargeted (`reversingMode`), and stops 32 ms
/// after it arrives.
#[test]
fn smoothed_animation_motion() {
    same_as_qt(
        include_str!("animation_audit/smooth.qml"),
        include_str!("animation_audit/smooth.qt612.txt"),
    );
}

/// Finding 34: `started`, `runningChanged`, `stopped` and `finished` come in Qt's order inside
/// the call that caused them; `pause()`, `resume()`, `complete()`, `restart()` and
/// `alwaysRunToEnd` do what Qt does, and a nested animation refuses them with a warning.
#[test]
fn animation_signals_and_pause() {
    let src = include_str!("animation_audit/sig.qml");
    same_as_qt(src, include_str!("animation_audit/sig.qt612.txt"));
    // What Qt 6.12 printed between those lines, the file's own path in place of Qt's.
    let file = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/animation_audit/sig.qml");
    let mut rt = Runtime::new(None);
    let root = rt.load_file(std::path::Path::new(file)).unwrap();
    rt.settle();
    rt.run_later();
    for i in 1..=60 {
        rt.tick(i as f64 * 16.0);
        rt.call_method(root, "sample", Vec::new()).unwrap().unwrap();
        rt.settle();
    }
    let not_running = format!(
        "file://{file}:25:5: QML NumberAnimation: setPaused() cannot be used when animation isn't running."
    );
    let nested = format!(
        "file://{file}:36:9: QML NumberAnimation: setRunning() cannot be used on non-root animation nodes."
    );
    assert_eq!(rt.errors(), [not_running.clone(), not_running, nested]);
}

/// Inside a frame the animations step in the order they started, and each write reaches its
/// `on<Prop>Changed` before the next animation steps: a handler sees the others where the
/// frame has got to. The children of a ParallelAnimation and the properties of one animation
/// go in their order, a Timer fires after the animations, a paused one does not step, and
/// one restarted, resumed or started again steps last.
#[test]
fn frame_steps_one_animation_at_a_time() {
    same_as_qt(
        include_str!("animation_audit/frame_order.qml"),
        include_str!("animation_audit/frame_order.qt612.txt"),
    );
}

/// A state change with a Transition: `stateChanged` comes before anything is written, the
/// Transition of the last change stops and the new one starts, the values it does not
/// animate are written one at a time with their handlers, and its end writes the last
/// values before `running` goes false.
#[test]
fn state_change_order_with_transition() {
    same_as_qt(
        include_str!("animation_audit/state_order.qml"),
        include_str!("animation_audit/state_order.qt612.txt"),
    );
}

/// A state change without a Transition writes its values one at a time, and a handler that
/// sets the state of another group changes that one there and then.
#[test]
fn state_change_from_a_handler() {
    same_as_qt(
        include_str!("animation_audit/state_nested.qml"),
        include_str!("animation_audit/state_nested.qt612.txt"),
    );
}

/// The order of the writes of one state: object by object, of a PropertyChanges the plain
/// values and then the bindings, each last one first, an extended state's before its own,
/// and after them what goes back to the values from before.
#[test]
fn state_change_write_order() {
    same_as_qt(
        include_str!("animation_audit/state_lists.qml"),
        include_str!("animation_audit/state_lists.qt612.txt"),
    );
}

/// `<Animation> on <property>` is a property value source: it runs by itself from its
/// component's completion unless `running: false` is written (a binding that is false then
/// counts), in a Loader and a delegate too; a `PropertyAction on` has acted by then.
#[test]
fn value_source_runs_by_itself() {
    same_as_qt(
        include_str!("animation_audit/value_source.qml"),
        include_str!("animation_audit/value_source.qt612.txt"),
    );
}

/// `SpringAnimation` moves as QSpringAnimation does: a spring stepped every 16 ms (`spring`,
/// `damping`, `mass`, the speed capped by `velocity`, at rest within `epsilon`), a straight
/// run at `velocity` when there is no spring, no move at all with neither; round a `modulus`
/// the short way; and its speed is kept when it is retargeted or stopped and started again.
#[test]
fn spring_animation_motion() {
    same_as_qt(
        include_str!("animation_audit/spring.qml"),
        include_str!("animation_audit/spring.qt612.txt"),
    );
}

/// `Vector3dAnimation` goes from one vector to the other in `float`s, and a
/// `PropertyAnimation` does the same for every type QVariantAnimation interpolates
/// (`vector2d`, `vector3d`, `vector4d`, `point`, `size`, `rect`, a `quaternion` by slerp).
#[test]
fn vector_animation_interpolates() {
    same_as_qt(
        include_str!("animation_audit/vec3.qml"),
        include_str!("animation_audit/vec3.qt612.txt"),
    );
}

/// An animation of reals (NumberAnimation, SmoothedAnimation, SpringAnimation) writes an
/// `int` property rounded; a PropertyAnimation interpolates the `int`s, cut off.
#[test]
fn int_property_takes_reals_rounded() {
    same_as_qt(
        include_str!("animation_audit/intp.qml"),
        include_str!("animation_audit/intp.qt612.txt"),
    );
}

/// `stop()` of an animation whose end is not known beforehand (a SmoothedAnimation, a
/// SpringAnimation, a group with one, `loops: Animation.Infinite`) says finished() and then
/// stopped(); one with a duration says stopped() alone.
#[test]
fn stop_without_a_known_end_says_finished() {
    same_as_qt(
        include_str!("animation_audit/stopfin.qml"),
        include_str!("animation_audit/stopfin.qt612.txt"),
    );
}

/// The same for an `int` a C++ type declares (`Timer.interval`, `Text.maximumLineCount`,
/// `Grid.columns`) and for an `int` member of a value type (`font.pixelSize`): rounded
/// under a NumberAnimation, cut off under a PropertyAnimation (`font.weight`).
#[test]
fn builtin_int_takes_reals_rounded() {
    same_as_qt(
        include_str!("animation_audit/intb.qml"),
        include_str!("animation_audit/intb.qt612.txt"),
    );
}

/// The animation a Behavior runs says `running` true before its first value and false after
/// its last one, each inside the write that the Behavior took (QQuickBehavior::write starts
/// the job, whose state change is QQuickAbstractAnimation::notifyRunningChanged). A new
/// target of one with a known duration says nothing; of a SmoothedAnimation, false and true.
#[test]
fn behavior_says_running_around_its_values() {
    same_as_qt(
        include_str!("animation_audit/beh.qml"),
        include_str!("animation_audit/beh.qt612.txt"),
    );
}

/// The first step of an animation `start()`, `restart()` or `running: true` began writes one
/// value at a time inside that call: a handler of the value sees `running` true, and one of
/// no duration says `stopped`, `running` false and `finished` after its last value.
#[test]
fn first_step_writes_one_value_at_a_time() {
    same_as_qt(
        include_str!("animation_audit/beh0.qml"),
        include_str!("animation_audit/beh0.qt612.txt"),
    );
}
