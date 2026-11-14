import QtQuick
Item {
    id: outer
    width: 100
    component Foo: Item {
        Connections { target: parent; function onWidthChanged() { console.log("fired, target width", target.width) } }
    }
    Foo { id: f; width: 10 }
    Component.onCompleted: { f.width = 20; console.log("--"); outer.width = 200 }
}
