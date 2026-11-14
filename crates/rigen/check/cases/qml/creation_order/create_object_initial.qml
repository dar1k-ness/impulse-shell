// QQmlComponent::createObject(parent, properties): the object is populated with its handlers
// connected, the parent hears childrenChanged, the initial properties are written (the
// handlers of both document layers hear them), then the bindings are enabled and
// Component.onCompleted runs. A child declared in the document is added before the parent's
// handlers are connected.
// qt: [ children base.a=5 own.a base.b=4 own.b t own.g base.e=50 base.done own.done ]
import QtQuick
Item {
    id: root
    property string result: Log.s
    onChildrenChanged: Log.add("children")
    Item { }
    Component {
        id: comp
        Base {
            property int g: a + 1
            onAChanged: Log.add("own.a")
            onBChanged: Log.add("own.b")
            onGChanged: Log.add("own.g")
            Component.onCompleted: Log.add("own.done")
        }
    }
    Component.onCompleted: {
        Log.add("[")
        comp.createObject(root, { b: 4, a: 5, tag: "t" })
        Log.add("]")
    }
}
