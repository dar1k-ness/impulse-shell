// Any Item has a `Layout` attached object, whether or not its document names it.
// qt: l m=3;=3;=3;="object";
import QtQuick
import QtQuick.Layouts
Item {
    id: root
    property string result: ""
    property var log: []
    function l(s) { root.log.push(s) }
    RowLayout { Item { id: la; Layout.onMarginsChanged: l("m"); Layout.onLeftMarginChanged: l("l") } Item { id: lb } Item { id: lc; Layout.margins: 1 } }
    function step(f) { var v; try { v = f() } catch (e) { v = "E:" + e.message } result += log.join(" ") + "=" + JSON.stringify(v) + ";"; log = [] }
    Component.onCompleted: {
        step(() => { la.Layout.margins = 3; return la.Layout.leftMargin })
        step(() => { lb.Layout.margins = 3; return lb.Layout.leftMargin })
        step(() => { lc.Layout.margins = 3; return lc.Layout.leftMargin })
        step(() => typeof la.Layout)
    }
}
