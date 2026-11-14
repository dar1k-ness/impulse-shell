// A handler of a signal of a grouped object, set on the item that has the group
// (`border.onWidthChanged`, `anchors.onMarginsChanged`, `layer.onEnabledChanged`,
// `drag.onTargetChanged`, `section.onPropertyChanged`, `xAxis.onEnabledChanged`) or on an
// attached object (`Layout`, `Keys`, `LayoutMirroring`, `KeyNavigation`), runs as in Qt 6.12:
// for a change only, with `this` the item, and next to an alias to the same property.
// Beside them, what the groups themselves send: `border.colorChanged` of a Rectangle comes for
// the same colour too (QQuickPen::setColor compares nothing); every side of a BorderImage's
// `border` sends `borderChanged` after its own signal; the margin of a side of `anchors` that
// nothing set is `margins`, and follows it (the sides first: right, left, top, bottom) until it
// is written, and again once it is reset.
// Each step: the signals it sent, in order, then its value.
// Not here: a group nothing set (`drag`, `section`, `xAxis`, `visibleArea`) reads null from a
// script, so each group has one property set; a handler on a property of a value type
// (`font.onPixelSizeChanged`) is a load error, see `value_type_handler*.qml`.
// qt: tf b4w a4rm a4m="init";bw3true abw=3;=3;bw4true abw=4;bc="#ff0000";bc="#ff0000";alm am=2;=2;alm=1;af=true;af al=0;le=true;b2w a2tm=0;bil bib=3;=3;dt dax=0;sp="a";lfw lpw lfh=true;=0;ke kp lme knt=0;b3c b3p=0;a4rm a4m="7 9 9";=9;a4m="7 9 4";a4rm=4;xe xm=5;
import QtQuick
import QtQuick.Layouts
Item {
    id: root
    property string result: ""
    property var log: []
    Rectangle {
        id: r
        border.width: 2
        border.onWidthChanged: root.log.push("bw" + border.width + (this === r))
        border.onColorChanged: root.log.push("bc")
        anchors.onMarginsChanged: root.log.push("am")
        anchors.onLeftMarginChanged: root.log.push("alm")
        anchors.onFillChanged: root.log.push("af")
        anchors.onLeftChanged: root.log.push("al")
        layer.onEnabledChanged: root.log.push("le")
        onColorChanged: root.log.push("color")
    }
    Rectangle {
        id: r2
        border {
            width: 1
            onWidthChanged: root.log.push("b2w")
        }
        anchors {
            onTopMarginChanged: root.log.push("a2tm")
        }
    }
    BorderImage { id: bi; border.onLeftChanged: root.log.push("bil"); border.onBorderChanged: root.log.push("bib") }
    MouseArea { id: ma; drag.threshold: 3; drag.onTargetChanged: root.log.push("dt"); drag.onAxisChanged: root.log.push("dax") }
    ListView { id: lv; section.criteria: ViewSection.FirstCharacter; section.onPropertyChanged: root.log.push("sp") }
    DragHandler { id: dh; xAxis.minimum: 1; xAxis.onEnabledChanged: root.log.push("xe"); xAxis.onMaximumChanged: root.log.push("xm") }
    RowLayout {
        Item { id: li; Layout.onFillWidthChanged: root.log.push("lfw"); Layout.onPreferredWidthChanged: root.log.push("lpw"); Layout.fillHeight: true; Layout.onFillHeightChanged: root.log.push("lfh") }
    }
    Item { id: ki; Keys.enabled: true; KeyNavigation.priority: KeyNavigation.BeforeItem; LayoutMirroring.childrenInherit: false; Keys.onEnabledChanged: root.log.push("ke"); Keys.onPriorityChanged: root.log.push("kp"); LayoutMirroring.onEnabledChanged: root.log.push("lme"); KeyNavigation.onTabChanged: root.log.push("knt") }
    Text { id: tx; font.pixelSize: 10; onFontChanged: root.log.push("tf") }
    Rectangle { id: r3; border.width: 2; border.color: "blue"; border.onWidthChanged: root.log.push("b3w"); border.onColorChanged: root.log.push("b3c"); border.pixelAligned: false; border.onPixelAlignedChanged: root.log.push("b3p") }
    Rectangle { id: r4; border.onWidthChanged: root.log.push("b4w"); border.width: 1 + 1; anchors.onMarginsChanged: root.log.push("a4m"); anchors.margins: 1 + 2; anchors.leftMargin: 7; anchors.onLeftMarginChanged: root.log.push("a4lm"); anchors.onRightMarginChanged: root.log.push("a4rm") }
    property alias bw: r.border.width
    onBwChanged: log.push("abw")
    function step(f) {
        var v
        try { v = f() } catch (e) { v = "E:" + e.message }
        result += log.join(" ") + "=" + JSON.stringify(v) + ";"
        log = []
    }
    Component.onCompleted: {
        step(() => "init")
        step(() => { r.border.width = 3; return r.border.width })
        step(() => { r.border.width = 3; return r.border.width })
        step(() => { bw = 4; return r.border.width })
        step(() => { r.border.color = "red"; return "" + r.border.color })
        step(() => { r.border.color = "red"; return "" + r.border.color })
        step(() => { r.anchors.margins = 2; return r.anchors.margins })
        step(() => { r.anchors.margins = 2; return r.anchors.leftMargin })
        step(() => { r.anchors.leftMargin = 1; return r.anchors.leftMargin })
        step(() => { r.anchors.fill = root; return r.anchors.fill === root })
        step(() => { r.anchors.fill = undefined; r.anchors.left = root.left; return 0 })
        step(() => { r.layer.enabled = true; return r.layer.enabled })
        step(() => { r2.border.width = 5; r2.anchors.topMargin = 3; return 0 })
        step(() => { bi.border.left = 3; return bi.border.left })
        step(() => { bi.border.left = 3; return bi.border.left })
        step(() => { ma.drag.target = r; ma.drag.axis = Drag.XAxis; return 0 })
        step(() => { lv.section.property = "a"; return lv.section.property })
        step(() => { li.Layout.fillWidth = true; li.Layout.preferredWidth = 5; li.Layout.fillHeight = false; return li.Layout.fillWidth })
        step(() => { li.Layout.fillWidth = true; return 0 })
        step(() => { ki.Keys.enabled = false; ki.Keys.priority = Keys.AfterItem; ki.LayoutMirroring.enabled = true; ki.KeyNavigation.tab = r; return 0 })
        step(() => { r3.border.width = 2; r3.border.color = "blue"; r3.border.pixelAligned = true; return 0 })
        step(() => { r4.anchors.margins = 9; return r4.anchors.leftMargin + " " + r4.anchors.rightMargin + " " + r4.anchors.topMargin })
        step(() => { r4.anchors.rightMargin = 9; return r4.anchors.rightMargin })
        step(() => { r4.anchors.margins = 4; return r4.anchors.leftMargin + " " + r4.anchors.rightMargin + " " + r4.anchors.topMargin })
        step(() => { r4.anchors.rightMargin = undefined; return r4.anchors.rightMargin })
        step(() => { dh.xAxis.enabled = false; dh.xAxis.maximum = 5; return dh.xAxis.maximum })
    }
}
