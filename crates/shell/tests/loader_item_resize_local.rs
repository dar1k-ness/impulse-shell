//! The item of a Loader with no size of its own changes size (the fill of the original's
//! `StyledProgressBar`, a `Rectangle` in a `Loader`, while the bar's value animates):
//! QQuickLoaderPrivate::_q_updateSize gives the Loader its item's size and nothing else in the
//! window is laid out again. A frame of the animation places the Loader, not the whole window,
//! and leaves every item where a full pass puts it.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;

const QML: &str = r##"
import QtQuick
import QtQuick.Layouts
import Rustshell
FloatingWindow {
    id: win
    width: 400; height: 300
    property real value: 0.2
    property var fill: fillLoader.item
    property var loader: fillLoader
    property var rest: restFill
    property var follower: tail
    property int laidOut: 0
    ColumnLayout {
        anchors.fill: parent
        Repeater {
            model: 6
            Rectangle {
                Layout.fillWidth: true
                implicitHeight: 20
                onYChanged: win.laidOut++
                onWidthChanged: win.laidOut++
            }
        }
        Item {
            id: bar
            Layout.fillWidth: true
            implicitHeight: 8
            Loader {
                id: fillLoader
                active: true
                sourceComponent: Rectangle {
                    anchors.left: parent.left
                    width: bar.width * win.value
                    height: bar.height
                }
            }
            Rectangle {
                id: tail
                anchors.left: fillLoader.right
                width: 4; height: 4
            }
            Rectangle {
                id: restFill
                anchors.right: parent.right
                width: (1 - win.value) * parent.width
                height: parent.height
            }
        }
    }
}
"##;

const SIZE: [f64; 2] = [400.0, 300.0];

/// The shell's frame (`App::draw_frame`): a full pass when the layout revision moved or a resize
/// is not local, else local passes. Returns (full passes, local passes, local targets).
fn frame(
    rt: &mut Runtime,
    fonts: &mut scene::Fonts,
    win: usize,
    laid: &mut u64,
) -> (usize, usize, Vec<usize>) {
    let mut local = 0;
    let mut targets = vec![];
    for pass in 0..4 {
        let mut rs = std::mem::take(&mut rt.world.borrow_mut().resized);
        let mut w = rt.world.borrow_mut();
        if w.layout_rev != *laid || !rs.iter().all(|&i| scene::is_local_resize(&mut w, i)) {
            drop(w);
            scene::layout_window(rt, fonts, win, SIZE);
            *laid = rt.world.borrow().layout_rev;
            rt.world.borrow_mut().resized.clear();
            return (1, local, targets);
        }
        if rs.is_empty() {
            break;
        }
        for &i in &rs {
            let t = scene::local_target(&mut w, i);
            if !targets.contains(&t) {
                targets.push(t);
            }
        }
        let rev = w.layout_rev;
        scene::measure_targets(&mut w, fonts, &rs);
        w.layout_rev = rev;
        drop(w);
        rt.settle();
        let mut w = rt.world.borrow_mut();
        let answered = std::mem::take(&mut w.resized);
        if !answered.is_empty()
            && (w.layout_rev != rev || !answered.iter().all(|&i| scene::is_local_resize(&mut w, i)))
        {
            w.resized = answered;
            continue;
        }
        rs.extend(answered);
        let rev = w.layout_rev;
        scene::relayout_targets(&mut w, fonts, &rs, pass);
        w.layout_rev = rev;
        drop(w);
        local += 1;
        rt.settle();
    }
    (0, local, targets)
}

fn item(rt: &mut Runtime, win: usize, name: &str) -> usize {
    match rt.world.borrow_mut().read(win, name) {
        Some(Value::Item(i)) => i,
        v => panic!("{name}: {v:?}"),
    }
}

#[test]
#[cfg_attr(
    not(feature = "interp"),
    ignore = "inline QML; the AOT-only build runs only the compiled QML tree"
)]
fn a_frame_of_a_resizing_loader_item_places_the_loader_alone() {
    let mut rt = Runtime::new(None);
    let win = rt.load_str(QML).unwrap();
    let mut fonts = scene::Fonts::default();
    rt.settle();
    scene::layout_window(&mut rt, &mut fonts, win, SIZE);
    let mut laid = rt.world.borrow().layout_rev;
    // What the first layout still answers.
    for _ in 0..3 {
        frame(&mut rt, &mut fonts, win, &mut laid);
    }
    let (fill, loader) = (item(&mut rt, win, "fill"), item(&mut rt, win, "loader"));
    let (rest, tail) = (item(&mut rt, win, "rest"), item(&mut rt, win, "follower"));
    let before = rt.get(win, "laidOut").to_number();
    let (mut full, mut local) = (0, 0);
    let mut targets = vec![];
    for step in 1..=6 {
        rt.set(win, "value", Value::Num(0.2 + 0.1 * step as f64));
        rt.settle();
        let (f, l, ts) = frame(&mut rt, &mut fonts, win, &mut laid);
        full += f;
        local += l;
        for t in ts {
            if !targets.contains(&t) {
                targets.push(t);
            }
        }
    }
    assert_eq!(full, 0, "no frame lays the whole window out");
    assert!(local >= 6, "each frame is a local pass: {local}");
    assert!(
        targets.contains(&loader),
        "the Loader is placed for its item {fill}: {targets:?}"
    );
    assert!(
        targets.iter().all(|t| [loader, rest, tail].contains(t)),
        "only the Loader, what follows it and the other fill are placed: {targets:?}"
    );
    assert_eq!(
        rt.get(win, "laidOut").to_number(),
        before,
        "the rows above stay as they are"
    );
    let geom = |rt: &mut Runtime| {
        [fill, loader, rest, tail]
            .map(|i| ["x", "y", "width", "height"].map(|k| rt.get(i, k).to_number()))
    };
    let got = geom(&mut rt);
    assert_eq!(got[0][2], 320.0, "the fill is 0.8 of the bar");
    assert_eq!(got[1][2], 320.0, "the Loader takes its item's width");
    assert_eq!(
        got[3][0], 320.0,
        "what is anchored to the Loader follows it"
    );
    scene::layout_window(&mut rt, &mut fonts, win, SIZE);
    assert_eq!(got, geom(&mut rt), "a full pass changes nothing");
}
