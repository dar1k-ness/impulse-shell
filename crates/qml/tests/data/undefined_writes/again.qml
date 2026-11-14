import QtQuick
import QtQuick.Controls
import QtQuick.Effects
Item {
    id: root
    function tt(f) { try { var v = f(); return typeof v + ":" + v } catch (e) { return "!" + e } }
    function un(o, p) { try { o[p] = undefined; return "" } catch (e) { return "!" + e } }
    // 0: the binding gives a value, 1: `undefined`, 2: the value again.
    property int step: 0
    Item { id: other; objectName: "other" }
    Control { id: c1; padding: 5; leftPadding: root.step == 1 ? undefined : 3; verticalPadding: root.step == 1 ? undefined : 4 }
    Popup { id: p1; margins: 6; leftMargin: root.step == 1 ? undefined : 1 }
    Popup { id: p2; margins: root.step == 1 ? undefined : 6 }
    Rectangle { id: r1; radius: 5; topLeftRadius: root.step == 1 ? undefined : 2 }
    RectangularShadow { id: s1; radius: 5; topLeftRadius: root.step == 1 ? undefined : 2 }
    ComboBox { id: cb1; model: ["a", "b"]; displayText: root.step == 1 ? undefined : "x" }
    Item { id: owner; objectName: "owner"; Popup { id: p3; parent: root.step == 1 ? undefined : other } }
    // The same by assignments.
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
    property var log: []
    Component.onCompleted: {
        var l = [bound()];
        step = 1; l.push(bound());
        step = 2; l.push(bound());
        c2.leftPadding = 3; p4.leftMargin = 1; r2.topLeftRadius = 2; cb2.displayText = "x"; p5.parent = other;
        l.push(assigned());
        l.push([un(c2, "leftPadding"), un(p4, "leftMargin"), un(r2, "topLeftRadius"), un(cb2, "displayText"), un(p5, "parent")].join(","));
        l.push(assigned());
        c2.leftPadding = 3; p4.leftMargin = 1; r2.topLeftRadius = 2; cb2.displayText = "x"; p5.parent = other;
        l.push(assigned());
        log = l;
    }
    property int tick: 0
    Timer { interval: 1; running: true; onTriggered: root.tick++ }
    property var cases: [
        () => root.log[0],
        () => root.log[1],
        () => root.log[2],
        () => root.log[3],
        () => root.log[4],
        () => root.log[5],
        () => root.log[6]
    ]
    property string result: (tick, cases.map(f => tt(f)).join(" | "))
}
