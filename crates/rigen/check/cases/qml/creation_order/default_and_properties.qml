// Objects of the default property are created in file order, after the objects bound to
// named properties, which are created last in the file first.
// qt: q p c1 c2 c3 base.done base.done base.done base.done base.done
import QtQuick
Item {
    property string result: Log.s
    Base { tag: "c1" }
    property Item p: Base { tag: "p" }
    Base { tag: "c2" }
    property Item q: Base { tag: "q" }
    Base { tag: "c3" }
}
