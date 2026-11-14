// The Basic style binds `horizontalPadding` of a Button and `leftPadding` of a TextField to
// their `padding`: the binding writes at `paddingChanged`, before setPadding tells of the
// sides that changed, so those are told of twice; after a write or a reset of the bound one
// only setPadding tells.
// qt: =[9,9,7,7];p l r h aw t l r b v h ah aw=[5,5,3,3];=[5,5,3,3];l r h aw=[9,9,3,3];p t b v ah=[9,9,5,5];l r h aw=[5,5,5,5];p t l r b v h ah aw=[4,4,4,4];l aw p t r b v h ah aw=[1,6,6,6];=[93,38];p l r h aw t l r b v h ah aw=[5,5,3,3];p t l r b v h ah aw=[3,3,3,3];p l t r b=[7,3,3,3];l p t r b=[1,5,5,5];l=[5,5,5,5];p t l r b=[2,2,2,2];p l t r b=[7,3,3,3];t p l r b=[9,5,1,5];p l r b=[4,0,1,0];
import QtQuick
import QtQuick.Controls
Item {
    id: root
    property string result: ""
    property var log: []
    function l(s) { root.log.push(s) }
    Button { id: bt; width: 100; height: 50; padding: 7; onPaddingChanged: l("p"); onLeftPaddingChanged: l("l"); onRightPaddingChanged: l("r"); onTopPaddingChanged: l("t"); onBottomPaddingChanged: l("b"); onHorizontalPaddingChanged: l("h"); onVerticalPaddingChanged: l("v"); onAvailableWidthChanged: l("aw"); onAvailableHeightChanged: l("ah") }
    DelayButton { id: db; width: 100; height: 50; onPaddingChanged: l("p"); onLeftPaddingChanged: l("l"); onRightPaddingChanged: l("r"); onTopPaddingChanged: l("t"); onBottomPaddingChanged: l("b"); onHorizontalPaddingChanged: l("h"); onVerticalPaddingChanged: l("v"); onAvailableWidthChanged: l("aw"); onAvailableHeightChanged: l("ah") }
    RoundButton { id: rb; width: 100; height: 50; onPaddingChanged: l("p"); onLeftPaddingChanged: l("l"); onRightPaddingChanged: l("r"); onTopPaddingChanged: l("t"); onBottomPaddingChanged: l("b"); onHorizontalPaddingChanged: l("h"); onVerticalPaddingChanged: l("v"); onAvailableWidthChanged: l("aw"); onAvailableHeightChanged: l("ah") }
    TextField { id: tf; width: 100; height: 50; onPaddingChanged: l("p"); onLeftPaddingChanged: l("l"); onRightPaddingChanged: l("r"); onTopPaddingChanged: l("t"); onBottomPaddingChanged: l("b") }
    TextArea { id: ta; width: 100; height: 50; onPaddingChanged: l("p"); onLeftPaddingChanged: l("l"); onRightPaddingChanged: l("r"); onTopPaddingChanged: l("t"); onBottomPaddingChanged: l("b") }
    function step(f) { var v; try { v = f() } catch (e) { v = "E:" + e.message } result += log.join(" ") + "=" + JSON.stringify(v) + ";"; log = [] }
    function all(o) { return [o.leftPadding, o.rightPadding, o.topPadding, o.bottomPadding] }
    Component.onCompleted: {
        log = []
        step(() => all(bt))
        step(() => { bt.padding = 3; return all(bt) })
        step(() => { bt.padding = 3; return all(bt) })
        step(() => { bt.horizontalPadding = 9; return all(bt) })
        step(() => { bt.padding = 5; return all(bt) })
        step(() => { bt.horizontalPadding = undefined; return all(bt) })
        step(() => { bt.padding = 4; return all(bt) })
        step(() => { bt.leftPadding = 1; bt.padding = 6; return all(bt) })
        step(() => [bt.availableWidth, bt.availableHeight])
        step(() => { db.padding = 3; return all(db) })
        step(() => { rb.padding = 3; return all(rb) })
        step(() => { tf.padding = 3; return all(tf) })
        step(() => { tf.leftPadding = 1; tf.padding = 5; return all(tf) })
        step(() => { tf.leftPadding = undefined; return all(tf) })
        step(() => { tf.padding = 2; return all(tf) })
        step(() => { ta.padding = 3; return all(ta) })
        step(() => { ta.topPadding = 1; ta.padding = 5; return all(ta) })
        step(() => { ta.padding = undefined; return all(ta) })
    }
}
