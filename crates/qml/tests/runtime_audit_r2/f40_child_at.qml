import QtQuick
Item {
    id: r; width: 100; height: 100
    function n(o) { return o ? o.objectName : o }
    Item { objectName: "a"; width: 50; height: 50 }
    Item { objectName: "b"; x: 25; y: 25; width: 50; height: 50
        Item { objectName: "b1"; x: 10; y: 10; width: 10; height: 10 } }
    Item { objectName: "hidden"; x: 60; y: 60; width: 40; height: 40; visible: false }
    Item { objectName: "empty"; x: 90 }
    QtObject { objectName: "q" }
    property Item pv: Item { objectName: "pv"; width: 100; height: 100 }
    Component.onCompleted: {
        console.log(n(childAt(0, 0)), n(childAt(30, 30)), n(childAt(49, 49)), n(childAt(74, 74)), n(childAt(75, 75)))
        console.log(n(childAt(80, 80)), n(childAt(90, 0)), n(childAt(-1, 0)), n(childAt(24.5, 24.5)), n(childAt(25, 10)))
        console.log(n(children[1].childAt(10, 10)), n(children[1].childAt(20, 20)), n(children[1].childAt(35, 35)), typeof childAt)
        console.log(n(childAt("3", "3")))
        try { childAt(1) } catch (e) { console.log(e.name, e.message) }
        try { childAt() } catch (e) { console.log(e.name, e.message) }
    }
}
