//! A size an animation writes is a set size: the animation writes through the item's setter
//! (QQuickItem::setWidth, `widthValid`), so the layout that follows keeps it and does not put
//! the implicit size back. ReloadPopup.qml's countdown bar is such an item: it has no `width`
//! of its own, only the one its animation runs from the popup's width down to nothing.

use qsg::raster::Canvas;
use shell::scene::{self, Target};

const QML: &str = r##"import QtQuick
import Rustshell
FloatingWindow {
    width: 200; height: 100
    color: "#222222"
    function go() { anim.start() }
    Rectangle {
        id: rect
        anchors.centerIn: parent
        color: "#ffD1E8D5"
        implicitWidth: 160; implicitHeight: 60
        Rectangle {
            id: bar
            color: "#ff0C1F13"
            anchors.bottom: parent.bottom
            anchors.left: parent.left
            anchors.margins: 10
            height: 5
            PropertyAnimation {
                id: anim
                target: bar
                property: "width"
                from: rect.width - bar.anchors.margins * 2
                to: 0
                duration: 2500
            }
        }
    }
}
"##;

/// The dark pixels of the bar's middle row in the window's picture at `now`.
fn bar(rt: &mut qml::runtime::Runtime, fonts: &mut scene::Fonts, win: usize, now: f64) -> usize {
    rt.tick(now);
    for _ in 0..3 {
        rt.settle();
        scene::layout_window(rt, fonts, win, [200.0, 100.0]);
    }
    let (restructured, moved, touched) = {
        let mut w = rt.world.borrow_mut();
        let w = &mut *w;
        (
            std::mem::take(&mut w.restructured),
            std::mem::take(&mut w.moved),
            std::mem::take(&mut w.touched),
        )
    };
    {
        let w = rt.world.borrow();
        scene::invalidate_parents(&w, &restructured);
        scene::invalidate_moved(&w, &moved);
        scene::invalidate_layers(&w, &touched);
    }
    let mut c = Canvas::new(200, 100, [0, 0, 0, 0]);
    scene::paint_window(rt, fonts, win, &mut Target::Cpu(&mut c));
    (0..200).filter(|x| c.px[67 * 200 + x][0] < 25).count()
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_width_an_animation_writes_stays_through_the_layout() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("scene.qml");
    std::fs::write(&file, QML).unwrap();
    let mut rt = qml::runtime::Runtime::new(Some(dir.path().to_path_buf()));
    let win = rt.load_file(&file).expect("scene loads");
    let mut fonts = scene::Fonts::default();
    assert_eq!(
        bar(&mut rt, &mut fonts, win, 1000.0),
        0,
        "before the countdown"
    );
    rt.call_method(win, "go", Vec::new()).unwrap().unwrap();
    // 140 px over 2500 ms, a fifth of the way each time.
    let widths: Vec<usize> = (1..=5)
        .map(|k| bar(&mut rt, &mut fonts, win, 1000.0 + 500.0 * k as f64))
        .collect();
    assert_eq!(widths, [112, 84, 56, 28, 0]);
}
