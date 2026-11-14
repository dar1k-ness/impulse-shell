// The change signals of the sides that follow a wider property (`anchors.margins`, the paddings
// of a Control, a Popup, a positioner and the text items, `Popup.margins`, `Layout.margins`),
// and `availableWidthChanged` / `availableHeightChanged` of a Control: which are sent, how many
// times and in what order, at a write, at a reset and when a binding gives a new value. The
// controls and the popup have a size of their own: one that follows its implicit size is told
// of its width again by the style's bindings.
// qt: r t b m p t l r b v h ah aw="init";r l t b m=0;r l b m=[3,4];l r b m=0;t=9;=9;p t l r b v h ah aw=[3,3,3,3];t b v ah=[3,3,5,5];l r h aw=[6,6,5,5];p=[6,6,5,5];t ah b v ah=[6,6,1,2];r h aw=[6,7,1,2];b v ah=[6,7,1,8];r h aw=[6,8,1,8];t ah=[6,8,8,8];l aw=[8,8,8,8];=[8,8,8,8];p t r b v h ah aw=[8,2,2,2];=[8,2,2,2];p t b v ah=[8,2,0,0];p t l r b h aw v ah=[3,3,3,3];t b v ah=[3,3,5,5];l r h aw=[6,6,5,5];t v ah p=[6,6,1,5];m mt ml mr mb=[3,3];m ml mr mb=[4,3];ml m mr mb=[7,3];mt=[7,7];m mt mr mb=[7,-1,-1];p t l r b=[3,3,3,3];p l r b=[4,4,3,4];l p r b=[9,9,3,9];t=[9,9,9,9];p t r b=[9,0,0,0];p t l r b=[3,3,3,3];p l r b=[4,4,3,4];l p r b=[9,9,3,9];t=[9,9,9,9];p t r b=[9,0,0,0];p t l r b=[3,3,3,3];p l r b=[4,4,3,4];l p r b=[9,9,3,9];t=[9,9,9,9];p t l r b=[3,3,3,3];p l r b=[4,4,3,4];l p r b=[9,9,3,9];t=[9,9,9,9];l t r b m=3;l r b m=[4,3];l r b m=[9,3];t=[9,9];r t b m p t l r b v h ah aw=[[5,5,5,5],1,5];m mt ml=2;ml m mt=[9,3];l h aw r h aw=[4,4,1,5];l h aw=[5,4,1,5];aw=120;l aw=100;ah aw=[0,0];ah=[0,100];
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
Item {
    id: root
    property string result: ""
    property var log: []
    function l(s) { root.log.push(s) }
    Item { id: a; anchors.onRightMarginChanged: l("r"); anchors.onLeftMarginChanged: l("l"); anchors.onTopMarginChanged: l("t"); anchors.onBottomMarginChanged: l("b"); anchors.onMarginsChanged: l("m") }
    Control { id: c; width: 100; height: 50; onPaddingChanged: l("p"); onLeftPaddingChanged: l("l"); onRightPaddingChanged: l("r"); onTopPaddingChanged: l("t"); onBottomPaddingChanged: l("b"); onHorizontalPaddingChanged: l("h"); onVerticalPaddingChanged: l("v"); onAvailableWidthChanged: l("aw"); onAvailableHeightChanged: l("ah") }
    Popup { id: p; width: 100; height: 50; onPaddingChanged: l("p"); onLeftPaddingChanged: l("l"); onRightPaddingChanged: l("r"); onTopPaddingChanged: l("t"); onBottomPaddingChanged: l("b"); onHorizontalPaddingChanged: l("h"); onVerticalPaddingChanged: l("v"); onMarginsChanged: l("m"); onLeftMarginChanged: l("ml"); onRightMarginChanged: l("mr"); onTopMarginChanged: l("mt"); onBottomMarginChanged: l("mb"); onAvailableWidthChanged: l("aw"); onAvailableHeightChanged: l("ah") }
    Column { id: col; onPaddingChanged: l("p"); onLeftPaddingChanged: l("l"); onRightPaddingChanged: l("r"); onTopPaddingChanged: l("t"); onBottomPaddingChanged: l("b") }
    Text { id: tx; onPaddingChanged: l("p"); onLeftPaddingChanged: l("l"); onRightPaddingChanged: l("r"); onTopPaddingChanged: l("t"); onBottomPaddingChanged: l("b") }
    TextInput { id: ti; onPaddingChanged: l("p"); onLeftPaddingChanged: l("l"); onRightPaddingChanged: l("r"); onTopPaddingChanged: l("t"); onBottomPaddingChanged: l("b") }
    TextEdit { id: te; onPaddingChanged: l("p"); onLeftPaddingChanged: l("l"); onRightPaddingChanged: l("r"); onTopPaddingChanged: l("t"); onBottomPaddingChanged: l("b") }
    RowLayout { Item { id: la; Layout.onMarginsChanged: l("m"); Layout.onLeftMarginChanged: l("l"); Layout.onRightMarginChanged: l("r"); Layout.onTopMarginChanged: l("t"); Layout.onBottomMarginChanged: l("b") } }
    property real pad: 2
    Control { id: cb; width: 100; height: 50; padding: root.pad; onPaddingChanged: l("p"); onLeftPaddingChanged: l("l"); onRightPaddingChanged: l("r"); onTopPaddingChanged: l("t"); onBottomPaddingChanged: l("b"); onHorizontalPaddingChanged: l("h"); onVerticalPaddingChanged: l("v"); onAvailableWidthChanged: l("aw"); onAvailableHeightChanged: l("ah") }
    Item { id: ab; anchors.margins: root.pad; anchors.leftMargin: 1; anchors.onRightMarginChanged: l("r"); anchors.onLeftMarginChanged: l("l"); anchors.onTopMarginChanged: l("t"); anchors.onBottomMarginChanged: l("b"); anchors.onMarginsChanged: l("m") }
    ToolTip { id: tt; margins: 4; onMarginsChanged: l("m"); onLeftMarginChanged: l("ml"); onTopMarginChanged: l("mt") }
    Control { id: sized; width: 100; height: 50; onAvailableWidthChanged: l("aw"); onAvailableHeightChanged: l("ah"); onLeftPaddingChanged: l("l") }
    function step(f) { var v; try { v = f() } catch (e) { v = "E:" + e.message } result += log.join(" ") + "=" + JSON.stringify(v) + ";"; log = [] }
    function all(o) { return [o.leftPadding, o.rightPadding, o.topPadding, o.bottomPadding] }
    Component.onCompleted: {
        step(() => "init")
        // anchors
        step(() => { a.anchors.margins = 3; return 0 })
        step(() => { a.anchors.topMargin = 3; a.anchors.margins = 4; return [a.anchors.topMargin, a.anchors.leftMargin] })
        step(() => { a.anchors.leftMargin = 9; a.anchors.margins = 9; return 0 })
        step(() => { a.anchors.topMargin = undefined; return a.anchors.topMargin })
        step(() => { a.anchors.leftMargin = undefined; return a.anchors.leftMargin })
        // Control
        step(() => { c.padding = 3; return all(c) })
        step(() => { c.verticalPadding = 5; return all(c) })
        step(() => { c.horizontalPadding = 6; return all(c) })
        step(() => { c.padding = 8; return all(c) })
        step(() => { c.topPadding = 1; c.verticalPadding = 2; return all(c) })
        step(() => { c.leftPadding = 6; c.horizontalPadding = 7; return all(c) })
        step(() => { c.verticalPadding = undefined; return all(c) })
        step(() => { c.horizontalPadding = undefined; return all(c) })
        step(() => { c.topPadding = undefined; return all(c) })
        step(() => { c.leftPadding = undefined; return all(c) })
        step(() => { c.leftPadding = 8; return all(c) })
        step(() => { c.padding = 2; return all(c) })
        step(() => { c.horizontalPadding = 2; return all(c) })
        step(() => { c.padding = undefined; return all(c) })
        // Popup
        step(() => { p.padding = 3; return all(p) })
        step(() => { p.verticalPadding = 5; return all(p) })
        step(() => { p.horizontalPadding = 6; return all(p) })
        step(() => { p.topPadding = 1; p.padding = 9; return all(p) })
        step(() => { p.margins = 3; return [p.leftMargin, p.topMargin] })
        step(() => { p.topMargin = 3; p.margins = 4; return [p.leftMargin, p.topMargin] })
        step(() => { p.leftMargin = 7; p.margins = 7; return [p.leftMargin, p.topMargin] })
        step(() => { p.topMargin = undefined; return [p.leftMargin, p.topMargin] })
        step(() => { p.margins = undefined; return [p.leftMargin, p.topMargin, p.margins] })
        // positioner, text items
        step(() => { col.padding = 3; return all(col) })
        step(() => { col.topPadding = 3; col.padding = 4; return all(col) })
        step(() => { col.leftPadding = 9; col.padding = 9; return all(col) })
        step(() => { col.topPadding = undefined; return all(col) })
        step(() => { col.padding = undefined; return all(col) })
        step(() => { tx.padding = 3; return all(tx) })
        step(() => { tx.topPadding = 3; tx.padding = 4; return all(tx) })
        step(() => { tx.leftPadding = 9; tx.padding = 9; return all(tx) })
        step(() => { tx.topPadding = undefined; return all(tx) })
        step(() => { tx.padding = undefined; return all(tx) })
        step(() => { ti.padding = 3; return all(ti) })
        step(() => { ti.topPadding = 3; ti.padding = 4; return all(ti) })
        step(() => { ti.leftPadding = 9; ti.padding = 9; return all(ti) })
        step(() => { ti.topPadding = undefined; return all(ti) })
        step(() => { te.padding = 3; return all(te) })
        step(() => { te.topPadding = 3; te.padding = 4; return all(te) })
        step(() => { te.leftPadding = 9; te.padding = 9; return all(te) })
        step(() => { te.topPadding = undefined; return all(te) })
        // Layout
        step(() => { la.Layout.margins = 3; return la.Layout.leftMargin })
        step(() => { la.Layout.topMargin = 3; la.Layout.margins = 4; return [la.Layout.leftMargin, la.Layout.topMargin] })
        step(() => { la.Layout.leftMargin = 9; la.Layout.margins = 9; return [la.Layout.leftMargin, la.Layout.topMargin] })
        step(() => { la.Layout.topMargin = undefined; return [la.Layout.leftMargin, la.Layout.topMargin] })
        // bound, a tooltip, a sized control
        step(() => { pad = 5; return [all(cb), ab.anchors.leftMargin, ab.anchors.topMargin] })
        step(() => { tt.margins = 2; return tt.leftMargin })
        step(() => { tt.leftMargin = 9; tt.margins = 3; return [tt.leftMargin, tt.topMargin] })
        step(() => { p.leftPadding = 4; p.rightPadding = 4; p.horizontalPadding = 1; return all(p) })
        step(() => { p.leftPadding = 5; return all(p) })
        step(() => { sized.width = 120; return sized.availableWidth })
        step(() => { sized.leftPadding = 20; return sized.availableWidth })
        step(() => { sized.padding = 200; return [sized.availableWidth, sized.availableHeight] })
        step(() => { sized.height = 500; return [sized.availableWidth, sized.availableHeight] })
    }
}
