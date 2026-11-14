//! Finding 21 of the second QML engine audit against Qt 6.12: an `AnchorAnimation` in a
//! Transition moves the `AnchorChanges` targets from where they were to where the new anchors
//! put them (only the targets it names, with its easing and duration; one reversed half way
//! sets off from where it is), and without one they move when the Transition ends. The probe
//! is stepped on a 16 ms clock with a layout pass per frame and has to give the lines Qt 6.12
//! printed on the same clock (`anchor.qt612.txt`, written by a `QAnimationDriver` harness).

use qml::runtime::Runtime;
use shell::scene;

#[test]
fn anchor_animation_moves_as_qt() {
    let mut rt = Runtime::new(None);
    let mut fonts = scene::Fonts::default();
    let root = rt
        .load_str(include_str!("anchor_animation/anchor.qml"))
        .unwrap();
    let size = [400.0, 300.0];
    rt.settle();
    rt.run_later();
    scene::layout_window(&mut rt, &mut fonts, root, size);
    rt.settle();
    for i in 1..=60 {
        rt.tick(i as f64 * 16.0);
        scene::layout_window(&mut rt, &mut fonts, root, size);
        rt.settle();
        rt.call_method(root, "sample", Vec::new()).unwrap().unwrap();
        rt.settle();
    }
    let log = rt.get(root, "log").to_js_string();
    let qt = include_str!("anchor_animation/anchor.qt612.txt");
    let (ours, qt): (Vec<&str>, Vec<&str>) = (log.lines().collect(), qt.lines().collect());
    for (i, (a, b)) in ours.iter().zip(qt.iter()).enumerate() {
        assert_eq!(a, b, "line {} (ours, then Qt 6.12)", i + 1);
    }
    assert_eq!(ours.len(), qt.len(), "number of lines");
}
