// A Repeater's delegate told to a container while it is made (QQuickRepeater::initItem sets
// its parent before its bindings are enabled): its model item is there, so a read runs the
// pending binding of a property with `index`, `modelData` and the roles; a BINDABLE property
// (`objectName`, `x`) has its default until the object is finalized.
// qt: lv count 1 ;lv item |qu|1|0|u|0.5|t;made l0u|qu|1|5|u|0.5|tl0u;lrep added 0 l0u|qu|1|5|u|0.5|tl0u;lv count 2 l0u|qu|1|5|u|0.5|tl0u;made l1v|qv|3|6|v|0.4|tl1v;lrep added 1 l1v|qv|3|6|v|0.4|tl1v;rv count 1 ;rv item |q0|1|0|a|0.5|t;made r0|q0|1|5|a|0.5|tr0;rrep added 0 r0|q0|1|5|a|0.5|tr0;rv count 2 r0|q0|1|5|a|0.5|tr0;made r1|q1|3|6|b|0.4|tr1;rrep added 1 r1|q1|3|6|b|0.4|tr1;sv count 1 ;sv item |q0|1|0|0|0.5|t;made p0|q0|1|5|0|0.5|tp0;rep added 0 p0|q0|1|5|0|0.5|tp0;sv count 2 p0|q0|1|5|0|0.5|tp0;made p1|q1|3|6|1|0.4|tp1;rep added 1 p1|q1|3|6|1|0.4|tp1
import QtQuick
import QtQuick.Controls
Item {
    id: root
    property string result: ""
    property var seen: []
    function d(it) {
        return it ? [it.objectName, it.nm, it.k, it.x, it.md, it.opacity, it.tag].join("|") : null
    }
    SwipeView {
        id: sv
        onCountChanged: root.seen.push(["sv count", count, root.d(currentItem)].join(" "))
        onCurrentItemChanged: root.seen.push(["sv item", root.d(currentItem)].join(" "))
        Repeater {
            model: 2
            Item {
                objectName: "p" + index
                property string nm: "q" + index
                property int k: index * 2 + 1
                property var md: modelData
                property string tag: "t" + objectName
                x: index + 5
                opacity: 0.5 - index / 10
                Component.onCompleted: root.seen.push(["made", root.d(this)].join(" "))
            }
            onItemAdded: (i, it) => root.seen.push(["rep added", i, root.d(it)].join(" "))
        }
    }
    SwipeView {
        id: rv
        onCountChanged: root.seen.push(["rv count", count, root.d(currentItem)].join(" "))
        onCurrentItemChanged: root.seen.push(["rv item", root.d(currentItem)].join(" "))
        Repeater {
            model: ["a", "b"]
            Item {
                required property int index
                required property string modelData
                objectName: "r" + index
                property string nm: "q" + index
                property int k: index * 2 + 1
                property var md: modelData
                property string tag: "t" + objectName
                x: index + 5
                opacity: 0.5 - index / 10
                Component.onCompleted: root.seen.push(["made", root.d(this)].join(" "))
            }
            onItemAdded: (i, it) => root.seen.push(["rrep added", i, root.d(it)].join(" "))
        }
    }
    ListModel { id: lm; ListElement { name: "u" } ListElement { name: "v" } }
    SwipeView {
        id: lv
        onCountChanged: root.seen.push(["lv count", count, root.d(currentItem)].join(" "))
        onCurrentItemChanged: root.seen.push(["lv item", root.d(currentItem)].join(" "))
        Repeater {
            model: lm
            Item {
                objectName: "l" + index + name
                property string nm: "q" + model.name
                property int k: index * 2 + 1
                property var md: name
                property string tag: "t" + objectName
                x: index + 5
                opacity: 0.5 - index / 10
                Component.onCompleted: root.seen.push(["made", root.d(this)].join(" "))
            }
            onItemAdded: (i, it) => root.seen.push(["lrep added", i, root.d(it)].join(" "))
        }
    }
    Component.onCompleted: result = seen.join(";")
}
