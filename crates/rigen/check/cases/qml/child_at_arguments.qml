// Item.childAt(x, y): the arguments are numbers (qreal); what other values and fewer
// arguments give, and which children a Repeater, a Loader and a non-Item child add.
// qt: strings a;bool a;null a;undefined null;nan null;infinity null;word null;object null;one Error: Insufficient arguments;none Error: Insufficient arguments;point Error: Insufficient arguments;typeof function;made cell0,cell1,cell2,loader,null,null;children 5;later moved;reparented first;back moved;created created;hidden moved;resized first
import QtQuick
Item {
    id: root
    width: 100; height: 100
    property string result: ""
    function n(i) { return i === null ? "null" : i === undefined ? "undefined" : i.objectName }
    function call(f) { try { return n(f()) } catch (e) { return String(e) } }
    Item {
        id: box; width: 100; height: 100
        Item { objectName: "a"; width: 50; height: 50 }
    }
    Item {
        id: made; width: 100; height: 100
        QtObject { objectName: "plain" }
        Repeater {
            objectName: "repeater"
            model: 3
            Item { objectName: "cell" + index; x: index * 20; width: 20; height: 20 }
        }
        Loader { objectName: "loader"; x: 60; width: 30; height: 30; sourceComponent: Item { objectName: "loaded" } }
    }
    Item {
        id: later; width: 100; height: 100
        Item { objectName: "first"; width: 50; height: 50 }
        Item { id: moved; objectName: "moved"; width: 50; height: 50 }
        Component { id: extra; Item { objectName: "created"; width: 50; height: 50 } }
    }
    Component.onCompleted: {
        var out = []
        out.push("strings " + call(() => box.childAt("10", "10")))
        out.push("bool " + call(() => box.childAt(true, true)))
        out.push("null " + call(() => box.childAt(null, null)))
        out.push("undefined " + call(() => box.childAt(undefined, undefined)))
        out.push("nan " + call(() => box.childAt(NaN, 10)))
        out.push("infinity " + call(() => box.childAt(Infinity, 10)))
        out.push("word " + call(() => box.childAt("x", 10)))
        out.push("object " + call(() => box.childAt({}, 10)))
        out.push("one " + call(() => box.childAt(10)))
        out.push("none " + call(() => box.childAt()))
        out.push("point " + call(() => box.childAt(Qt.point(10, 10))))
        out.push("typeof " + typeof box.childAt)
        out.push("made " + [made.childAt(5, 5), made.childAt(25, 5), made.childAt(45, 5), made.childAt(65, 5), made.childAt(95, 5), made.childAt(5, 50)].map(n).join(","))
        out.push("children " + made.children.length)
        out.push("later " + n(later.childAt(10, 10)))
        moved.parent = root
        out.push("reparented " + n(later.childAt(10, 10)))
        moved.parent = later
        out.push("back " + n(later.childAt(10, 10)))
        var c = extra.createObject(later)
        out.push("created " + n(later.childAt(10, 10)))
        c.visible = false
        out.push("hidden " + n(later.childAt(10, 10)))
        moved.width = 5
        out.push("resized " + n(later.childAt(10, 10)))
        result = out.join(";")
    }
}
