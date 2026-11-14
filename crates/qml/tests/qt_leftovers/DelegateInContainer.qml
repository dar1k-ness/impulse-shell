// A Repeater's delegate told to a container while it is made (QQuickRepeater::initItem sets
// its parent before its bindings are enabled): its model item is there, so a read runs the
// pending binding of a property with `index`, `modelData` and the roles; a BINDABLE property
// (`objectName`, `x`) has its default until the object is finalized.
import QtQuick
import QtQuick.Controls
Item {
    id: root
    function d(it) {
        return it ? [it.objectName, it.nm, it.k, it.x, it.md, it.opacity, it.tag].join("|") : null
    }
    SwipeView {
        id: sv
        onCountChanged: console.log("sv count", count, root.d(currentItem))
        onCurrentItemChanged: console.log("sv item", root.d(currentItem))
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
                Component.onCompleted: console.log("made", root.d(this))
            }
            onItemAdded: (i, it) => console.log("rep added", i, root.d(it))
        }
    }
    SwipeView {
        id: rv
        onCountChanged: console.log("rv count", count, root.d(currentItem))
        onCurrentItemChanged: console.log("rv item", root.d(currentItem))
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
                Component.onCompleted: console.log("made", root.d(this))
            }
            onItemAdded: (i, it) => console.log("rrep added", i, root.d(it))
        }
    }
    ListModel { id: lm; ListElement { name: "u" } ListElement { name: "v" } }
    SwipeView {
        id: lv
        onCountChanged: console.log("lv count", count, root.d(currentItem))
        onCurrentItemChanged: console.log("lv item", root.d(currentItem))
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
                Component.onCompleted: console.log("made", root.d(this))
            }
            onItemAdded: (i, it) => console.log("lrep added", i, root.d(it))
        }
    }
    Component.onCompleted: Qt.quit()
}
