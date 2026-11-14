// A binding that names the default property (`data: [...]`, `data: Obj`) is one of the
// objects of the default property: its objects are created at its place in the file among
// the declared children, after the objects bound to other properties.
// qt: k q p c1 x y c2 c3 d1 one d2 base.done base.done base.done base.done base.done base.done base.done base.done base.done base.done base.done
import QtQuick
Item {
    property string result: Log.s
    Item {
        property Item p: Base { tag: "p" }
        Base { tag: "c1" }
        data: [ Base { tag: "x" }, Base { tag: "y" } ]
        Base { tag: "c2" }
        property Item q: Base { tag: "q" }
        children: [ Base { tag: "k" } ]
        Base { tag: "c3" }
        onWidthChanged: Log.add("w")
        width: 5
    }
    Item {
        Base { tag: "d1" }
        data: Base { tag: "one" }
        Base { tag: "d2" }
    }
}
