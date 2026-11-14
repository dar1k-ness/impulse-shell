// The default property of an Item is looked up by its name, `data`: an Item that declares a
// property `data` gets its child objects in that property, as a declared list does, and they
// are not its children; a declared `data` that cannot hold them is a load error. Declaring
// `children` or `resources` changes nothing: the child objects stay children.
// qt: derived 2 0; 0 5:2: Cannot assign value of type "Item" to property "data", expecting "int"; 1 ok s0; 2 ok 2 0 false true; 3 5:2: Cannot assign object of type "QObject" to list property "data"; expected "QQuickItem"; 4 ok 2 0; 5 ok true 0; 6 ok true 0; 7 ok true 0; 8 ok 2 0; 9 5:2: Cannot assign object of type "QQuickItem" to list property "data"; expected "int"; 10 ok 5 true 2 1; 11 ok 0 true 1; 12 ok 5 true 1 2; 13 ok 0 1 2; 14 5:2: Cannot assign to non-existent default property; 15 ok 0 1; 16 ok 0 5 1
import QtQuick

Item {
    id: root
    property string result: ""
    component Base: Item {
        property list<QtObject> data
    }
    Base {
        id: derived
        Item {}
        QtObject {}
    }
    function load(body) {
        try {
            var o = Qt.createQmlObject("import QtQuick\n" + body, root, "inline.qml")
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
            "Item {\n id: o\n property int data: 5\n Item {}\n property string r: data\n}",
            "Item {\n id: o\n property string data: 's'\n property string r: data + children.length\n}",
            "Item {\n id: o\n property list<QtObject> data\n Item { id: k }\n QtObject {}\n property string r: data.length + ' ' + children.length + ' ' + (k.parent === o) + ' ' + (data[0] === k)\n}",
            "Item {\n id: o\n property list<Item> data\n QtObject {}\n property string r: data.length\n}",
            "Item {\n id: o\n property list<Item> data\n Item {}\n Item {}\n property string r: data.length + ' ' + children.length\n}",
            "Item {\n id: o\n property var data\n Item { id: k }\n property string r: (data === k) + ' ' + children.length\n}",
            "Item {\n id: o\n property Item data\n Item { id: k }\n property string r: (data === k) + ' ' + children.length\n}",
            "Item {\n id: o\n property Item data\n Item {}\n Item { id: k }\n property string r: (data === k) + ' ' + children.length\n}",
            "Item {\n id: o\n property list<QtObject> data: [QtObject {}]\n Item {}\n property string r: data.length + ' ' + children.length\n}",
            "Item {\n id: o\n property list<int> data\n Item {}\n property string r: data.length\n}",
            "Item {\n id: o\n property int children: 5\n Item { id: k }\n QtObject {}\n property string r: children + ' ' + (k.parent === o) + ' ' + data.length + ' ' + resources.length\n}",
            "Item {\n id: o\n property list<QtObject> children\n Item { id: k }\n property string r: children.length + ' ' + (k.parent === o) + ' ' + data.length\n}",
            "Item {\n id: o\n property int resources: 5\n Item { id: k }\n QtObject {}\n property string r: resources + ' ' + (k.parent === o) + ' ' + children.length + ' ' + data.length\n}",
            "Item {\n id: o\n property list<QtObject> resources\n Item {}\n QtObject {}\n property string r: resources.length + ' ' + children.length + ' ' + data.length\n}",
            "QtObject {\n id: o\n property list<QtObject> data\n QtObject {}\n property string r: data.length\n}",
            "Item {\n id: o\n default property list<QtObject> data\n Item {}\n property string r: data.length + ' ' + children.length\n}",
            "Item {\n id: o\n default property list<QtObject> stuff\n property int data: 5\n Item {}\n property string r: stuff.length + ' ' + data + ' ' + children.length\n}",
        ]
        var r = ["derived " + derived.data.length + " " + derived.children.length]
        for (var i = 0; i < cases.length; i++)
            r.push(i + " " + load(cases[i]))
        result = r.join("; ")
    }
}
