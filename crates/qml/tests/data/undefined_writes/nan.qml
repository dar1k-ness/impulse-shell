import QtQuick
Item {
    id: root
    function tt(f) { try { var v = f(); return typeof v + ":" + v } catch (e) { return "!" + e } }
    property int nw: 0
    property int nh: 0
    property int nb: 0
    property var und
    Item { id: a; width: root.und + 1; height: 0 / 0; onWidthChanged: root.nw++; onHeightChanged: root.nh++ }
    Item { id: b; property real src: NaN; width: src; implicitWidth: 6; onWidthChanged: root.nb++ }
    Item { id: c; property real src: NaN; width: src; implicitWidth: 2 }
    property var cases: [() => a.width, () => a.height, () => root.nw, () => root.nh, () => b.width, () => root.nb,
        () => { b.src = 4; return b.width }, () => root.nb, () => { b.src = NaN; return b.width }, () => root.nb,
        () => c.width, () => { c.implicitWidth = 8; return c.width }, () => { c.width = NaN; return c.width },
        () => { c.width = 3; c.width = NaN; return c.width }, () => { c.implicitWidth = 9; return c.width }]
    property string result
    Component.onCompleted: result = cases.map(tt).join(" | ")
}
