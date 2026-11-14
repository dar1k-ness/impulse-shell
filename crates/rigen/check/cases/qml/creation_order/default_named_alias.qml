// The default property of a type from QML is an alias: a binding that names it is created
// at its place in the file among the declared children, as for `data`.
// qt: box.first q c1 x c2 base.done base.done base.done base.done base.done
import QtQuick
Item {
    property string result: Log.s
    component Box: Item {
        default property alias content: inner.data
        property Item first: Base { tag: "box.first" }
        Item { id: inner }
    }
    Box {
        Base { tag: "c1" }
        content: [ Base { tag: "x" } ]
        Base { tag: "c2" }
        property Item q: Base { tag: "q" }
    }
}
