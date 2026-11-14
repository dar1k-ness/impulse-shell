import QtQuick
import QtQml.Models

// The handler of a delegate runs while the delegate is being made, after a child of it made an
// object in a binding (`createObject`): the delegate's own bindings are still pending, and the
// handler reads their values.
Item {
    id: root
    function tt(f) { try { var v = f(); return typeof v + ":" + v } catch (e) { return "!" + e } }
    property int currentIndex: 0
    property int base: 3
    property string seen: ""
    Component { id: maker; Item {} }
    ListModel { id: lm }
    Repeater { model: lm; delegate: Item {} }
    Loader {
        id: loader
        active: false
        sourceComponent: Row {
            Repeater {
                model: 2
                delegate: Item {
                    id: seg
                    required property int index
                    readonly property bool current: index === root.currentIndex
                    implicitWidth: label.implicitWidth + 28
                    property var v: root.base + 5
                    property int n: root.base + 5
                    property string s: "v" + root.base
                    onCurrentChanged: root.seen += [seg.n, seg.s].join(" ")
                    Item { id: label; implicitWidth: 10; property var made: maker.createObject(label) }
                }
            }
        }
    }
    Component.onCompleted: loader.active = true
    property int tick: 0
    Timer { interval: 1; running: true; onTriggered: root.tick++ }
    property var cases: [
        () => root.seen
    ]
    property string result: (tick, cases.map(f => tt(f)).join(" | "))
}
