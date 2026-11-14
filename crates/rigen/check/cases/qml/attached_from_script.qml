// A script's `x.Layout` (and any other attached type's name) is the attached object, made on
// the first read for an object nothing attached it to in the document; its properties read
// what the C++ class starts with, and the side margins follow `Layout.margins`.
// qt: object:QQuickLayoutAttached(0x) number:6 number:6 number:2 number:6 boolean:false number:-1 number:0 number:0 | object:QQuickLayoutAttached(0x) number:0 number:0 boolean:false number:Infinity number:0 number:1 boolean:true boolean:false | number:0 number:0 boolean:false number:0 object:QQuickLayoutAttached(0x) object:QQuickLayoutAttached(0x) object:QQmlComponentAttached(0x) undefined:undefined object:QtQuick.Layouts/RowLayout object:QtQuick/Item | number:6 number:3 number:6 number:8 number:8 number:1 boolean:true | object:QQuickLayoutAttached(0x) boolean:true object:QQuickKeysAttached(0x) boolean:true object:QQuickKeyNavigationAttached(0x) boolean:true object:QQuickToolTipAttached(0x) boolean:true object:QQmlComponentAttached(0x) boolean:true object:QQuickListViewAttached(0x) boolean:true object:QQuickGridViewAttached(0x) boolean:true object:QQuickStackViewAttached(0x) boolean:true object:QQuickSwipeViewAttached(0x) boolean:true object:QQuickDragAttached(0x) boolean:true object:QQuickLayoutMirroringAttached(0x) boolean:true object:QQuickAccessibleAttached(0x) boolean:true object:QQuickTextAreaAttached(0x) boolean:true object:QQuickWindowAttached(0x) boolean:true | boolean:true boolean:false string: number:700 object:null boolean:false object:null number:-1 boolean:false boolean:false object:null number:-1 | boolean:true boolean:true | 4 4 4 4 4 4 1 4 | 4 4 9 4 4 | -1 -1 9
import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
Item {
    id: root
    RowLayout {
        Item { id: li; Layout.margins: 6; Layout.rightMargin: 2 }
        Item { id: lj }
    }
    Item { id: plain }
    QtObject { id: obj }
    ListView { id: lv; width: 10; height: 10; model: 1; currentIndex: 0; delegate: Item {} }
    Menu { id: menu; margins: 4 }
    ToolTip { id: tip; margins: 4; leftMargin: 1 }
    readonly property var names: ["Layout", "Keys", "KeyNavigation", "ToolTip", "Component", "ListView", "GridView", "StackView",
                                  "SwipeView", "Drag", "LayoutMirroring", "Accessible", "TextArea", "Window"]
    function d(v) { return typeof v + ":" + String(v).replace(/0x[0-9a-f]+/, "0x") }
    function q(f) { try { return d(f()) } catch (x) { return String(x) } }
    property string result
    Component.onCompleted: {
        var l = []
        l.push([q(() => li.Layout), q(() => li.Layout.margins), q(() => li.Layout.leftMargin), q(() => li.Layout.rightMargin), q(() => li.Layout.topMargin),
                q(() => li.Layout.fillWidth), q(() => li.Layout.preferredWidth), q(() => li.Layout.alignment), q(() => li.Layout.row)].join(" "))
        l.push([q(() => lj.Layout), q(() => lj.Layout.margins), q(() => lj.Layout.leftMargin), q(() => lj.Layout.fillWidth), q(() => lj.Layout.maximumWidth),
                q(() => lj.Layout.minimumWidth), q(() => lj.Layout.columnSpan), q(() => lj.Layout === lj.Layout), q(() => lj.Layout === li.Layout)].join(" "))
        l.push([q(() => plain.Layout.margins), q(() => plain.Layout.leftMargin), q(() => plain.Layout.fillWidth), q(() => root.Layout.leftMargin),
                q(() => menu.Layout), q(() => obj.Layout), q(() => obj.Component), q(() => plain.Nothing), q(() => plain.RowLayout), q(() => plain.Item)].join(" "))
        li.Layout.leftMargin = 3; li.Layout.rightMargin = undefined; lj.Layout.margins = 8; lj.Layout.topMargin = 1; plain.Layout.fillWidth = true
        l.push([q(() => li.Layout.margins), q(() => li.Layout.leftMargin), q(() => li.Layout.rightMargin), q(() => lj.Layout.margins),
                q(() => lj.Layout.leftMargin), q(() => lj.Layout.topMargin), q(() => plain.Layout.fillWidth)].join(" "))
        l.push(names.map(k => q(() => plain[k]) + " " + q(() => plain[k] === plain[k])).join(" "))
        l.push([q(() => plain.Keys.enabled), q(() => plain.ToolTip.visible), q(() => plain.ToolTip.text), q(() => plain.ToolTip.delay), q(() => plain.ListView.view),
                q(() => plain.ListView.isCurrentItem), q(() => plain.StackView.view), q(() => plain.StackView.index), q(() => plain.Drag.active),
                q(() => plain.LayoutMirroring.enabled), q(() => plain.KeyNavigation.left), q(() => plain.SwipeView.index)].join(" "))
        l.push([q(() => lv.itemAtIndex(0).ListView.view === lv), q(() => lv.itemAtIndex(0).ListView.isCurrentItem)].join(" "))
        l.push([menu.margins, menu.leftMargin, menu.topMargin, menu.rightMargin, menu.bottomMargin, tip.margins, tip.leftMargin, tip.topMargin].join(" "))
        tip.leftMargin = undefined; menu.topMargin = 9
        l.push([menu.margins, menu.leftMargin, menu.topMargin, tip.margins, tip.leftMargin].join(" "))
        menu.margins = undefined
        l.push([menu.margins, menu.leftMargin, menu.topMargin].join(" "))
        result = l.join(" | ")
    }
}
