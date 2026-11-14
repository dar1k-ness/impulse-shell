// A value written after a reset is a set one again: a binding that gave a value, then
// `undefined`, then the value, and the same by assignments, for a padding, a popup's margins,
// a corner radius, a ComboBox's `displayText` and a popup's `parent`.
// qt: 3 4 1 6 6 2 2 x other | 5 5 6 -1 -1 5 5 a owner | 3 4 1 6 6 2 2 x other | 3 1 2 x other | 5 6 5 a owner2 | 3 1 2 x other
import QtQuick
import QtQuick.Controls
import QtQuick.Effects
Item {
    id: root
    property int step: 0
    Item { id: other; objectName: "other" }
    Control { id: c1; padding: 5; leftPadding: root.step == 1 ? undefined : 3; verticalPadding: root.step == 1 ? undefined : 4 }
    Popup { id: p1; margins: 6; leftMargin: root.step == 1 ? undefined : 1 }
    Popup { id: p2; margins: root.step == 1 ? undefined : 6 }
    Rectangle { id: r1; radius: 5; topLeftRadius: root.step == 1 ? undefined : 2 }
    RectangularShadow { id: s1; radius: 5; topLeftRadius: root.step == 1 ? undefined : 2 }
    ComboBox { id: cb1; model: ["a", "b"]; displayText: root.step == 1 ? undefined : "x" }
    Item { id: owner; objectName: "owner"; Popup { id: p3; parent: root.step == 1 ? undefined : other } }
    Control { id: c2; padding: 5 }
    Popup { id: p4; margins: 6 }
    Rectangle { id: r2; radius: 5 }
    ComboBox { id: cb2; model: ["a", "b"] }
    Item { id: owner2; objectName: "owner2"; Popup { id: p5 } }
    function bound() {
        return [c1.leftPadding, c1.topPadding, p1.leftMargin, p2.margins, p2.leftMargin,
                r1.topLeftRadius, s1.topLeftRadius, cb1.displayText, p3.parent.objectName].join(" ")
    }
    function assigned() {
        return [c2.leftPadding, p4.leftMargin, r2.topLeftRadius, cb2.displayText, p5.parent.objectName].join(" ")
    }
    function write() { c2.leftPadding = 3; p4.leftMargin = 1; r2.topLeftRadius = 2; cb2.displayText = "x"; p5.parent = other }
    property string result
    Component.onCompleted: {
        var l = [bound()]
        step = 1; l.push(bound())
        step = 2; l.push(bound())
        write(); l.push(assigned())
        c2.leftPadding = undefined; p4.leftMargin = undefined; r2.topLeftRadius = undefined; cb2.displayText = undefined; p5.parent = undefined
        l.push(assigned())
        write(); l.push(assigned())
        result = l.join(" | ")
    }
}
