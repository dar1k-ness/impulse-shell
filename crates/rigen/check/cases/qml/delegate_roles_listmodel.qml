// The roles of a ListModel in a delegate of a Repeater (Qt 6.12): `model.name` read by a
// binding of the delegate and of an inline component, `modelData` of a model of one role, a
// required role (the fixture of the Repeater order work).
// qt: inline(,x,0);required(aa);required(b);plain(aa);plain(b);inline(aa,aa,0);inline(b,b,1)
import QtQuick
Item {
    id: root
    property string result: ""
    property var seen: []
    ListModel { id: lm; ListElement { name: "aa" } ListElement { name: "b" } }
    component Tab: Item { property string name: model.name; property string md: "" + modelData; property int ix: index; Component.onCompleted: root.seen.push("inline(" + name + "," + md + "," + ix + ")") }
    Repeater { model: lm; Tab {} }
    Repeater { model: lm; Item { property string name: model.name; Component.onCompleted: root.seen.push("plain(" + name + ")") } }
    Repeater { model: lm; Item { required property string name; Component.onCompleted: root.seen.push("required(" + name + ")") } }
    Repeater { model: ["x"]; Tab {} }
    Component.onCompleted: result = seen.join(";")
}
