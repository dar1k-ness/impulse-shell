//! QQuickAnchors baseline: `anchors.baseline` lines up the item's baselineOffset with the
//! target's (`y = target.y + target.baselineOffset - baselineOffset + anchors.baselineOffset`),
//! and baselineOffset is the first line's baseline for Text/TextInput/TextEdit/Label (after
//! the vertical alignment offset) and `topPadding + contentItem.baselineOffset` for a Control
//! (SettingsSpinRow's value/unit), wherever its contentItem sits.
//! Expected numbers are Qt 6.12's (PySide6-Essentials 6.12.0, offscreen) for the same QML.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;

const QML: &str = r#"
import QtQuick
import QtQuick.Controls
Item {
    id: root_
    width: 400; height: 400
    property int unitSize: 12
    property real seen: t2.baselineOffset
    property var o_v: v
    property var o_u: unitText
    property var o_t2: t2
    property var o_r1: r1
    property var o_r2: r2
    property var o_r3: r3
    property var o_vc: vc
    property var o_ti: ti
    property var o_te: te
    property var o_lb: lb
    property var o_ct: ct
    property var o_ct2: ct2
    property var o_empty: empty
    property var o_plain: plain
    property var o_ca: ca
    property var o_tib: tib
    property var o_teb: teb
    property var o_te2b: te2b
    property var o_rtb: rtb
    property var o_rtc: rtc
    Item {
        width: 200; height: 36
        Row {
            anchors.centerIn: parent
            spacing: 2
            Text { id: v; anchors.baseline: unitText.baseline; text: "42"; font.family: "DejaVu Sans"; font.pixelSize: 15 }
            Text { id: unitText; anchors.verticalCenter: parent.verticalCenter; text: "px"; font.family: "DejaVu Sans"; font.pixelSize: root_.unitSize }
        }
    }
    Text { id: t2; y: 100; text: "x"; font.family: "DejaVu Sans"; font.pixelSize: 20; topPadding: 7 }
    Rectangle { id: r1; anchors.baseline: t2.baseline; anchors.baselineOffset: 3; width: 5; height: 5 }
    Rectangle { id: r2; baselineOffset: 4; anchors.baseline: t2.baseline; width: 5; height: 5 }
    Rectangle { id: r3; anchors.top: t2.bottom; anchors.baseline: t2.baseline; width: 5; height: 5 }
    Text { id: vc; y: 200; height: 100; text: "a"; font.family: "DejaVu Sans"; font.pixelSize: 20; verticalAlignment: Text.AlignVCenter }
    TextInput { id: ti; text: "abc"; font.family: "DejaVu Sans"; font.pixelSize: 16; topPadding: 2 }
    TextEdit { id: te; text: "abc"; font.family: "DejaVu Sans"; font.pixelSize: 16; topPadding: 3 }
    Label { id: lb; text: "abc"; font.family: "DejaVu Sans"; font.pixelSize: 16; topPadding: 5 }
    Control { id: ct; padding: 6; contentItem: Text { text: "q"; font.family: "DejaVu Sans"; font.pixelSize: 10 } }
    Control { id: ct2; padding: 6; contentItem: Item { baselineOffset: 3 } }
    Text { id: empty; font.family: "DejaVu Sans"; font.pixelSize: 20 }
    Item { id: plain }
    Control { id: ca; padding: 6; contentItem: Text { anchors.fill: parent; text: "q"; font.family: "DejaVu Sans"; font.pixelSize: 10 } }
    TextInput { id: tib; height: 100; verticalAlignment: TextInput.AlignBottom; text: "abc"; font.family: "DejaVu Sans"; font.pixelSize: 16 }
    TextEdit { id: teb; height: 100; verticalAlignment: TextEdit.AlignBottom; text: "abc"; font.family: "DejaVu Sans"; font.pixelSize: 16 }
    TextEdit { id: te2b; height: 100; topPadding: 3; bottomPadding: 5; verticalAlignment: TextEdit.AlignBottom; text: "abc\ndef"; font.family: "DejaVu Sans"; font.pixelSize: 16 }
    TextEdit { id: rtb; height: 100; verticalAlignment: TextEdit.AlignBottom; textFormat: TextEdit.RichText; text: "<b>abc</b>"; font.family: "DejaVu Sans"; font.pixelSize: 16 }
    TextEdit { id: rtc; height: 100; verticalAlignment: TextEdit.AlignVCenter; textFormat: TextEdit.RichText; text: "<b>abc</b>"; font.family: "DejaVu Sans"; font.pixelSize: 16 }
}
"#;

fn frame(rt: &mut Runtime, fonts: &mut scene::Fonts, root: usize, laid: &mut u64) {
    for _ in 0..4 {
        let rs = std::mem::take(&mut rt.world.borrow_mut().resized);
        let mut w = rt.world.borrow_mut();
        if w.layout_rev != *laid || !rs.iter().all(|&i| scene::is_local_resize(&mut w, i)) {
            drop(w);
            scene::layout_window(rt, fonts, root, [400.0, 400.0]);
            *laid = rt.world.borrow().layout_rev;
            rt.world.borrow_mut().resized.clear();
            return;
        }
        if rs.is_empty() {
            return;
        }
        let rev = w.layout_rev;
        scene::relayout_targets(&mut w, fonts, &rs, 0);
        w.layout_rev = rev;
        drop(w);
        rt.settle();
    }
}

/// Loads `QML`, lays it out like the shell's first frame, then applies each write in `steps`
/// followed by a frame, calling `probe` after the first frame and after every step.
fn run<T>(steps: &[(&str, Value)], mut probe: impl FnMut(&mut Runtime, usize) -> T) -> Vec<T> {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(QML).unwrap();
    rt.settle();
    let mut fonts = scene::Fonts::default();
    let mut laid = rt.world.borrow().layout_rev;
    scene::layout_window(&mut rt, &mut fonts, root, [400.0, 400.0]);
    laid = rt.world.borrow().layout_rev.max(laid);
    rt.settle();
    frame(&mut rt, &mut fonts, root, &mut laid);
    let mut out = vec![probe(&mut rt, root)];
    for (k, v) in steps {
        rt.world.borrow_mut().write(root, k, v.clone());
        rt.settle();
        frame(&mut rt, &mut fonts, root, &mut laid);
        out.push(probe(&mut rt, root));
    }
    out
}

fn item_num(rt: &mut Runtime, root: usize, item: &str, prop: &str) -> f64 {
    let mut w = rt.world.borrow_mut();
    let Some(Value::Item(i)) = w.read(root, &format!("o_{item}")) else {
        panic!("o_{item} is not an item");
    };
    w.read(i, prop).map_or(f64::NAN, |v| v.to_number())
}

const ITEMS: [&str; 20] = [
    "v", "u", "t2", "r1", "r2", "r3", "vc", "ti", "te", "lb", "ct", "ct2", "empty", "plain", "ca",
    "tib", "teb", "te2b", "rtb", "rtc",
];

/// Qt 6.12: (y, baselineOffset) of each of `ITEMS`.
const QT_FIRST: [(f64, f64); 20] = [
    (-0.796875, 13.921875),
    (2.0, 11.125),
    (100.0, 25.5625),
    (128.5625, 0.0),
    (121.5625, 4.0),
    (131.0, 0.0),
    (200.0, 56.5625),
    (0.0, 16.84375),
    (0.0, 17.84375),
    (0.0, 19.84375),
    (0.0, 15.28125),
    (0.0, 9.0),
    (0.0, 18.5625),
    (0.0, 0.0),
    (0.0, 15.28125),
    (0.0, 95.84375),
    (0.0, 95.84375),
    (0.0, 71.84375),
    (0.0, 95.84375),
    (0.0, 55.34375),
];

fn probe(rt: &mut Runtime, root: usize) -> Vec<(f64, f64)> {
    ITEMS
        .iter()
        .map(|n| {
            (
                item_num(rt, root, n, "y"),
                item_num(rt, root, n, "baselineOffset"),
            )
        })
        .collect()
}

fn assert_qt(got: &[(f64, f64)], want: &[(f64, f64)], when: &str) {
    for ((n, g), w) in ITEMS.iter().zip(got).zip(want) {
        assert!(
            (g.0 - w.0).abs() < 1e-3 && (g.1 - w.1).abs() < 1e-3,
            "{when}: {n} (y, baselineOffset) = {g:?}, Qt 6.12 has {w:?}"
        );
    }
}

#[test]
fn baseline_anchor_matches_qt() {
    let got = run(&[("unitSize", Value::Num(20.0))], |rt, root| {
        let seen = rt
            .world
            .borrow_mut()
            .read(root, "seen")
            .map(|v| v.to_number());
        (probe(rt, root), seen)
    });
    assert_qt(&got[0].0, &QT_FIRST, "first layout");
    assert_eq!(
        got[0].1,
        Some(25.5625),
        "a binding reads Text.baselineOffset"
    );
    // The unit text grows: the value text follows its baseline.
    let mut later = QT_FIRST;
    later[0] = (4.640625, 13.921875);
    later[1] = (0.0, 18.5625);
    assert_qt(&got[1].0, &later, "unit font grown");
}

/// Qt 6.12 `positionToRectangle(start).y` of each line of the AlignBottom editors: the lines sit
/// at the bottom of the padded area (QQuickTextUtil::alignedY), as painted and hit-tested.
#[test]
fn bottom_aligned_editors_lay_out_at_the_bottom() {
    let got = run(&[], |rt, root| {
        let mut fonts = scene::Fonts::default();
        ["tib", "teb", "te2b"].map(|n| {
            let i = {
                let mut w = rt.world.borrow_mut();
                let Some(Value::Item(i)) = w.read(root, &format!("o_{n}")) else {
                    panic!("o_{n} is not an item");
                };
                i
            };
            let mut w = rt.world.borrow_mut();
            scene::edit_geometry(&mut w, &mut fonts, i)
                .into_iter()
                .map(|(_, y, h, _)| (y, h))
                .collect::<Vec<_>>()
        })
    });
    let want: [&[(f64, f64)]; 3] = [
        &[(81.0, 19.0)],
        &[(81.0, 19.0)],
        &[(57.0, 19.0), (76.0, 19.0)],
    ];
    for ((n, g), w) in ["tib", "teb", "te2b"].iter().zip(&got[0]).zip(want) {
        assert_eq!(g.len(), w.len(), "{n}: lines {g:?}, Qt 6.12 has {w:?}");
        for (g, w) in g.iter().zip(w) {
            assert!(
                (g.0 - w.0).abs() < 1e-3 && (g.1 - w.1).abs() < 1e-3,
                "{n}: line (y, height) {g:?}, Qt 6.12 has {w:?}"
            );
        }
    }
}
