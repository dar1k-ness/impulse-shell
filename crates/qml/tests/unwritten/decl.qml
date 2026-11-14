// Methods Qt declares that this engine has not written: what `typeof` says of each and what
// a call gives, in Qt 6.12 and here. `Item.mapToItem` is written here and is the control.
import QtCore
import QtQuick

Item {
    id: root
    objectName: "root"
    width: 200
    height: 200

    Rectangle {
        id: inner
        objectName: "inner"
        x: 10
        y: 10
        width: 50
        height: 50
        activeFocusOnTab: true
    }
    Flickable {
        id: flick
        objectName: "flick"
        x: 100
        width: 100
        height: 100
        contentWidth: 400
        contentHeight: 400
    }
    ListView {
        id: list
        objectName: "list"
        y: 100
        width: 100
        height: 100
        model: 5
        delegate: Item {
            required property int index
            objectName: "row" + index
            width: 100
            height: 20
        }
    }
    Loader {
        id: loader
        objectName: "loader"
    }

    function show(v) {
        if (v === undefined || v === null)
            return String(v);
        if (typeof v === "object" && "objectName" in v)
            return "the object " + v.objectName;
        if (typeof v === "object" && "x" in v)
            return "(" + v.x + ", " + v.y + ")";
        return typeof v + " " + v;
    }
    function probe(label, o, name, call) {
        var line = label + ": typeof " + typeof o[name];
        try {
            line += ", returns " + show(call(o));
        } catch (e) {
            line += ", throws " + e.message;
        }
        console.log(line);
    }

    Component.onCompleted: {
        probe("Item.mapToItem", inner, "mapToItem", o => o.mapToItem(root, 1, 2));
        probe("Item.childAt", root, "childAt", o => o.childAt(20, 20));
        probe("Item.contains", inner, "contains", o => o.contains(Qt.point(5, 5)));
        probe("Item.nextItemInFocusChain", root, "nextItemInFocusChain", o => o.nextItemInFocusChain());
        probe("Flickable.flick", flick, "flick", o => o.flick(0, -500));
        probe("Flickable.cancelFlick", flick, "cancelFlick", o => o.cancelFlick());
        probe("Flickable.returnToBounds", flick, "returnToBounds", o => o.returnToBounds());
        probe("ListView.indexAt", list, "indexAt", o => o.indexAt(5, 25));
        probe("ListView.itemAt", list, "itemAt", o => o.itemAt(5, 25));
        probe("Loader.setSource", loader, "setSource", o => o.setSource(""));
        probe("Qt.inputMethod.reset", Qt.inputMethod, "reset", o => o.reset());
        probe("Qt.inputMethod.invokeAction", Qt.inputMethod, "invokeAction", o => o.invokeAction(Qt.inputMethod.Click, 0));
        probe("StandardPaths.destroy", StandardPaths, "destroy", o => o.destroy());
        probe("Easing.destroy", Easing, "destroy", o => o.destroy());
        Qt.quit();
    }
}
