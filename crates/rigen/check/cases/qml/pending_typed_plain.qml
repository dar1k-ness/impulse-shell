// A handler run while its object is created reads the object's own bound properties (`var`,
// `int`, `string`) and its `implicitWidth`: nothing is made before the read. `seen` is `typeof v`, `v`, `n`,
// `s`, `implicitWidth` as the handler saw them; Qt 6.12 runs each pending binding on the read.
// qt: string:number 8 8 v3 38
import QtQuick
import QtQml.Models
Item {
    id: root
    function tt(f) { try { var v = f(); return typeof v + ":" + v } catch (e) { return "!" + e } }
    property int currentIndex: 0
    property int base: 3
    property string seen: ""
    function look(seg) { return [typeof seg.v, seg.v, seg.n, seg.s, seg.implicitWidth].join(" ") }
    Component { id: maker; Item {} }
    ListModel { id: lm }
    Repeater { model: lm; delegate: Item {} }
    Loader { id: other }
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
                    onCurrentChanged: { root.seen += root.look(seg) }
                    Item { id: label; implicitWidth: 10;  }
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
