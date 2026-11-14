import QtQuick
import QtQuick.Controls

// The content size of a ScrollView (QQuickScrollView) and what it gives the Flickable it
// makes: one child of each kind, several children, a `contentWidth` that is set and one that
// is bound, `availableWidth`. `snap()` gives a row per view.
Item {
    id: root
    width: 400; height: 400

    property real cw: 300
    property real grow: 300

    // one TextArea: as declared, between the sides of `parent`, and too wide to fit
    ScrollView { id: t1; width: 200; height: 100; TextArea { id: t1k; font.family: "DejaVu Sans"; font.pixelSize: 14; text: "one\ntwo"; background: null } }
    ScrollView { id: t2; width: 200; height: 100; TextArea { id: t2k; anchors { left: parent.left; right: parent.right } font.family: "DejaVu Sans"; font.pixelSize: 14; text: "one\ntwo"; background: null } }
    ScrollView { id: t3; width: 200; height: 100; TextArea { id: t3k; font.family: "DejaVu Sans"; font.pixelSize: 14; text: "a line of text that is a good deal wider than the view\ntwo"; background: null } }
    // one Flickable: with a content size and with none
    ScrollView { id: f1; width: 200; height: 100; Flickable { id: f1k; contentWidth: 300; contentHeight: 150 } }
    ScrollView { id: f2; width: 200; height: 100; Flickable { id: f2k } }
    // one Item: with an implicit size, with a size only, with neither, filling `parent`
    ScrollView { id: i1; width: 200; height: 100; Item { id: i1k; implicitWidth: root.grow; implicitHeight: 150 } }
    ScrollView { id: i2; width: 200; height: 100; Item { id: i2k; width: 300; height: 150 } }
    ScrollView { id: i3; width: 200; height: 100; Item { id: i3k } }
    ScrollView { id: i4; width: 200; height: 100; Item { id: i4k; implicitWidth: 300; implicitHeight: 150; anchors.fill: parent } }
    // several children
    ScrollView { id: m1; width: 200; height: 100; Item { id: m1k; implicitWidth: 300; implicitHeight: 150 } Item { implicitWidth: 50; implicitHeight: 60 } }
    // contentWidth: set, bound to a property, bound to the child, bound to availableWidth
    ScrollView { id: c1; width: 200; height: 100; contentWidth: 300; contentHeight: 150; Item { id: c1k; implicitWidth: 50; implicitHeight: 60 } }
    ScrollView { id: c2; width: 200; height: 100; contentWidth: root.cw; Item { id: c2k; implicitWidth: 50; implicitHeight: 150 } }
    ScrollView { id: c3; width: 200; height: 100; contentWidth: c3k.width - 1; Item { id: c3k; width: 300; height: 150 } }
    ScrollView { id: c4; width: 200; height: 100; contentWidth: availableWidth; Item { id: c4k; implicitWidth: 300; implicitHeight: 150 } }
    // availableWidth: paddings, and a bar that is always there
    ScrollView { id: p1; width: 200; height: 100; padding: 8; Item { id: p1k; implicitWidth: 300; implicitHeight: 150 } }
    ScrollView { id: p2; width: 200; height: 100; ScrollBar.vertical.policy: ScrollBar.AlwaysOn; Item { id: p2k; implicitWidth: 300; implicitHeight: 150; anchors.fill: parent } }
    ScrollView { id: p3; width: 200; height: 100; ScrollBar.vertical.policy: ScrollBar.AlwaysOff; ScrollBar.horizontal.policy: ScrollBar.AlwaysOff; Item { id: p3k; implicitWidth: 300; implicitHeight: 150 } }

    property var views: ({
        t1: [t1, t1k], t2: [t2, t2k], t3: [t3, t3k], f1: [f1, f1k], f2: [f2, f2k],
        i1: [i1, i1k], i2: [i2, i2k], i3: [i3, i3k], i4: [i4, i4k], m1: [m1, m1k],
        c1: [c1, c1k], c2: [c2, c2k], c3: [c3, c3k], c4: [c4, c4k],
        p1: [p1, p1k], p2: [p2, p2k], p3: [p3, p3k]
    })

    function r(v) { return v === undefined || v === null ? "?" : Math.round(v * 100) / 100; }
    function box(o) { return o ? [r(o.x), r(o.y), r(o.width), r(o.height)].join(" ") : "none"; }
    function flick(f) {
        return f ? box(f) + " content " + r(f.contentWidth) + " " + r(f.contentHeight)
            + " item " + r(f.contentItem.width) + " " + r(f.contentItem.height) : "none";
    }
    function bar(o) { return o ? box(o) + " size " + r(o.size) + " visible " + o.visible : "none"; }
    function snap() {
        var out = [];
        for (var n in views) {
            var v = views[n][0], k = views[n][1], f = v.contentItem;
            var vb = v.ScrollBar.vertical, hb = v.ScrollBar.horizontal;
            out.push(n + ": content " + r(v.contentWidth) + " " + r(v.contentHeight)
                + " available " + r(v.availableWidth) + " " + r(v.availableHeight)
                + " implicit " + r(v.implicitWidth) + " " + r(v.implicitHeight)
                + " bars " + r(v.effectiveScrollBarWidth) + " " + r(v.effectiveScrollBarHeight)
                + " | flickable " + flick(f) + " | child " + box(k)
                + " | vertical " + bar(vb) + " | horizontal " + bar(hb));
        }
        return out.join("\n");
    }
    function change() {
        cw = 120;
        grow = 80;
        t1k.text = "a much longer first line of text\ntwo\nthree\nfour\nfive\nsix\nseven";
        c1.contentWidth = 90;
        i2k.width = 120;
    }
    // `contentWidth = undefined` is QQuickPane::resetContentWidth.
    function reset() {
        c1.contentWidth = undefined;
        c1.contentHeight = undefined;
        c2.contentWidth = undefined;
    }
}
