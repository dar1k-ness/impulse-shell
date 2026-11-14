import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// The content size of QQuickPane types. `snap()` gives, for each pane, contentWidth,
// contentHeight, implicitContentWidth, implicitContentHeight, implicitWidth, implicitHeight.
Item {
    id: root
    width: 400; height: 400

    // nothing in it
    Pane { id: pe } Frame { id: fe } GroupBox { id: ge } Page { id: pge } ScrollView { id: se }
    ScrollView { id: st; Timer {} }
    Pane { id: pt; Timer {} }
    // one child with an implicit size
    Pane { id: p1; Item { implicitWidth: 30; implicitHeight: 20 } }
    Frame { id: f1; Item { implicitWidth: 30; implicitHeight: 20 } }
    GroupBox { id: g1; Item { implicitWidth: 30; implicitHeight: 20 } }
    Page { id: pg1; Item { implicitWidth: 30; implicitHeight: 20 } }
    ScrollView { id: s1; Item { implicitWidth: 30; implicitHeight: 20 } }
    Pane { id: p1t; Item { implicitWidth: 30; implicitHeight: 20 } Timer {} }
    ScrollView { id: s1t; Item { implicitWidth: 30; implicitHeight: 20 } Timer {} }
    Pane { id: p1h; Item { implicitWidth: 30; implicitHeight: 20; visible: false } }
    ScrollView { id: s1h; Item { implicitWidth: 30; implicitHeight: 20; visible: false } }
    Pane { id: p1b; background: Rectangle { implicitWidth: 100; implicitHeight: 90 } Item { implicitWidth: 30; implicitHeight: 20 } }
    // one child with a size and no implicit size
    Pane { id: p2; Item { width: 30; height: 20 } }
    ScrollView { id: s2; Item { width: 30; height: 20 } }
    // more than one child
    Pane { id: p3; Item { implicitWidth: 30; implicitHeight: 20 } Item { implicitWidth: 50; implicitHeight: 60 } }
    Page { id: pg3; Item { implicitWidth: 30; implicitHeight: 20 } Item { implicitWidth: 50; implicitHeight: 60 } }
    ScrollView { id: s3; Item { implicitWidth: 30; implicitHeight: 20 } Item { implicitWidth: 50; implicitHeight: 60 } }
    ScrollView { id: s3a; Item { implicitWidth: 30; implicitHeight: 20 } Item {} }
    ScrollView { id: s3b; Item {} Item { implicitWidth: 30; implicitHeight: 20 } }
    ScrollView { id: s3c; Item { implicitWidth: root.grow; implicitHeight: root.grow } Item {} }
    ScrollView { id: s3d; Item { implicitWidth: 30; implicitHeight: 20 } Item {} Item {} }
    Pane { id: p3h; Item { implicitWidth: 30; implicitHeight: 20 } Item { visible: false } }
    Pane { id: pr2; Repeater { model: 2; Item { implicitWidth: 30; implicitHeight: 20 } } }
    Pane { id: pr1; Repeater { model: 1; Item { implicitWidth: 30; implicitHeight: 20 } } }
    ScrollView { id: sr2; Repeater { model: 2; Item { implicitWidth: 30; implicitHeight: 20 } } }
    ScrollView { id: sr1; Repeater { model: 1; Item { implicitWidth: 30; implicitHeight: 20 } } }
    // a set content size, nothing in it
    Pane { id: p4; contentWidth: 41; contentHeight: 42 }
    Page { id: pg4; contentWidth: 41; contentHeight: 42 }
    ScrollView { id: s4; contentWidth: 41; contentHeight: 42 }
    Pane { id: p4t; contentWidth: 41; contentHeight: 42; Timer {} }
    // a set content size and a child
    Pane { id: p5; contentWidth: 41; contentHeight: 42; Item { implicitWidth: 30; implicitHeight: 20 } }
    Frame { id: f5; contentWidth: 41; contentHeight: 42; Item { implicitWidth: 30; implicitHeight: 20 } }
    GroupBox { id: g5; contentWidth: 41; contentHeight: 42; Item { implicitWidth: 30; implicitHeight: 20 } }
    Page { id: pg5; contentWidth: 41; contentHeight: 42; Item { implicitWidth: 30; implicitHeight: 20 } }
    ScrollView { id: s5; contentWidth: 41; contentHeight: 42; Item { implicitWidth: 30; implicitHeight: 20 } }
    // a child whose implicit size changes
    property real grow: 10
    Pane { id: p6; Item { implicitWidth: root.grow; implicitHeight: root.grow + 1 } }
    ScrollView { id: s6; Item { implicitWidth: root.grow; implicitHeight: root.grow + 1 } }
    // a content item of its own
    Pane { id: p7; contentItem: Item { implicitWidth: 7; implicitHeight: 8 } }
    Pane { id: p8; contentItem: Item { Item { implicitWidth: 9; implicitHeight: 10 } } }
    Pane { id: p9; contentItem: Item { implicitWidth: 7; Item { implicitWidth: 9; implicitHeight: 10 } } }
    // a layout as the child
    Pane { id: p10; RowLayout { Item { implicitWidth: 11; implicitHeight: 12 } Item { implicitWidth: 13; implicitHeight: 14 } } }
    // a Flickable in a ScrollView
    ScrollView { id: s7; Flickable { contentWidth: 70; contentHeight: 80 } }
    ScrollView { id: s9; contentWidth: 41; Flickable { contentWidth: 70; contentHeight: 80 } }
    // padding
    Pane { id: p12; padding: 3; Item { implicitWidth: 30; implicitHeight: 20 } }
    Pane { id: p13; padding: 3; contentWidth: 41; Item { implicitWidth: 30; implicitHeight: 20 } }
    ScrollView { id: s12; padding: 4; Item { implicitWidth: 30; implicitHeight: 20 } }
    // a binding that turns undefined
    property bool on: true
    Pane { id: p14; contentWidth: root.on ? 41 : undefined; contentHeight: root.on ? 42 : undefined; Item { implicitWidth: 30; implicitHeight: 20 } }
    Pane { id: p15; contentWidth: root.on ? 41 : undefined; contentHeight: root.on ? 42 : undefined }
    // children that leave or hide
    Pane { id: d2; Item { id: d2a; implicitWidth: 30; implicitHeight: 20 } }
    Pane { id: d3; Item { implicitWidth: 30; implicitHeight: 20 } Item { id: d3b; implicitWidth: 50; implicitHeight: 60 } }
    Pane { id: d4; Item { id: d4a; implicitWidth: 30; implicitHeight: 20 } }
    ScrollView { id: d7; Item { id: d7a; implicitWidth: 30; implicitHeight: 20 } }

    property var panes: ({
        pe: pe, fe: fe, ge: ge, pge: pge, se: se, st: st, pt: pt,
        p1: p1, f1: f1, g1: g1, pg1: pg1, s1: s1, p1t: p1t, s1t: s1t, p1h: p1h, s1h: s1h, p1b: p1b,
        p2: p2, s2: s2,
        p3: p3, pg3: pg3, s3: s3, s3a: s3a, s3b: s3b, s3c: s3c, s3d: s3d, p3h: p3h,
        pr2: pr2, pr1: pr1, sr2: sr2, sr1: sr1,
        p4: p4, pg4: pg4, s4: s4, p4t: p4t,
        p5: p5, f5: f5, g5: g5, pg5: pg5, s5: s5,
        p6: p6, s6: s6, p7: p7, p8: p8, p9: p9, p10: p10, s7: s7, s9: s9,
        p12: p12, p13: p13, s12: s12, p14: p14, p15: p15,
        d2: d2, d3: d3, d4: d4, d7: d7
    })
    function snap() {
        var out = [];
        for (var n in panes) {
            var o = panes[n];
            out.push(n + ": " + [o.contentWidth, o.contentHeight, o.implicitContentWidth, o.implicitContentHeight, o.implicitWidth, o.implicitHeight].join(" "));
        }
        return out.join("\n");
    }
    // `contentWidth = undefined` is QQuickPane::resetContentWidth.
    function reset() {
        for (var o of [p4, pg4, s4, p4t, p5, f5, g5, pg5, s5, s9, p13, pe, p1]) {
            o.contentWidth = undefined;
            o.contentHeight = undefined;
        }
    }
    function change() {
        grow = 25;
        on = false;
        d2a.parent = root;
        d3b.parent = root;
        d4a.visible = false;
        d7a.parent = root;
    }
    function set() {
        for (var o of [pe, p1, se, s1, s2, p15]) {
            o.contentWidth = 51;
            o.contentHeight = 52;
        }
        on = true;
    }
}
