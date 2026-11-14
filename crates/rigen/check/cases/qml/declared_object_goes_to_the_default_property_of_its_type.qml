// An object declared inside another goes to the default property of the nearest C++ type that
// names one: `flickableData` for a Flickable, `data` again for ListView, GridView and PathView,
// `contentData` for a Pane (FINAL, so no property can be declared under it). A property declared
// in QML under that name takes the objects (or is the load error); one declared under the default property's name of another type does not.
// qt: flick 2; view 0 true; 0 ok true true; 1 ok true true; 2 6:2: Cannot assign value of type "Item" to property "flickableData", expecting "int"; 3 ok 5; 4 ok true true; 5 ok true true; 6 6:2: Cannot assign value of type "Item" to property "data", expecting "int"; 7 ok 5; 8 ok 2 true; 9 ok 5 true; 10 ok true true; 11 5:2: Cannot override FINAL property; 12 ok 5 true; 13 6:2: Cannot assign to non-existent default property; 14 ok true 0; 15 ok true 2
import QtQuick
import QtQuick.Controls

Item {
    id: root
    property string result: ""
    component Flick: Flickable {
        property list<QtObject> flickableData
    }
    Flick {
        id: flick
        Item {}
        QtObject {}
    }
    component View: ListView {
        property list<QtObject> flickableData
    }
    View {
        id: view
        Item { id: viewKid }
    }
    function load(body) {
        try {
            var o = Qt.createQmlObject("import QtQuick\nimport QtQuick.Controls\n" + body, root, "inline.qml")
            var r = "ok " + o.r
            o.destroy()
            return r
        } catch (e) {
            var m = /:(\d+:\d+: .*)$/.exec(String(e.message).trim())
            return m ? m[1] : String(e.message)
        }
    }
    Component.onCompleted: {
        var cases = [
            "Flickable {\n id: o\n property var flickableData\n Item { id: k }\n property string r: (flickableData === k) + ' ' + (k.parent === null)\n}",
            "Flickable {\n id: o\n property var data\n Item { id: k }\n property string r: (data === undefined) + ' ' + (k.parent !== null)\n}",
            "Flickable {\n id: o\n property int flickableData: 5\n Item {}\n property string r: flickableData\n}",
            "Flickable {\n id: o\n property int data: 5\n Item {}\n property string r: data\n}",
            "ListView {\n id: o\n property var data\n Item { id: k }\n property string r: (data === k) + ' ' + (k.parent === null)\n}",
            "ListView {\n id: o\n property var flickableData\n Item { id: k }\n property string r: (flickableData === undefined) + ' ' + (k.parent === o)\n}",
            "ListView {\n id: o\n property int data: 5\n Item {}\n property string r: data\n}",
            "ListView {\n id: o\n property int flickableData: 5\n Item {}\n property string r: flickableData\n}",
            "GridView {\n id: o\n property list<QtObject> data\n Item { id: k }\n QtObject {}\n property string r: data.length + ' ' + (k.parent === null)\n}",
            "GridView {\n id: o\n property int flickableData: 5\n Item { id: k }\n property string r: flickableData + ' ' + (k.parent === o)\n}",
            "PathView {\n id: o\n property var data\n Item { id: k }\n property string r: (data === k) + ' ' + (k.parent === null)\n}",
            "Pane {\n id: o\n property var contentData\n Item { id: k }\n property string r: (contentData === k) + ' ' + (k.parent === null)\n}",
            "Pane {\n id: o\n property int data: 5\n Item { id: k }\n property string r: data + ' ' + (k.parent !== null)\n}",
            "Timer {\n id: o\n property var data\n QtObject {}\n property string r: data\n}",
            "Repeater {\n id: o\n property var delegate\n Item { id: k }\n property string r: (delegate === k) + ' ' + count\n}",
            "Repeater {\n id: o\n property var data\n model: 2\n Item {}\n property string r: (data === undefined) + ' ' + count\n}",
        ]
        var r = ["flick " + flick.flickableData.length, "view " + view.flickableData.length + " " + (viewKid.parent === view)]
        for (var i = 0; i < cases.length; i++)
            r.push(i + " " + load(cases[i]))
        result = r.join("; ")
    }
}
