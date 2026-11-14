import QtQuick
Item {
    id: root
    property bool watching: true
    function say(s) { console.log(s) }
    function kind(o) { return o === null ? "null" : o === undefined ? "undefined" : o === root ? "root" : o === plain ? "plain" : o === timer ? "timer" : String(o).replace(/_QML.*|\(0x.*/, "") }
    function counts() { return "children " + root.children.length + " resources " + root.resources.length + " data " + root.data.length }
    Component { id: item; Item { property int n: 1; Component.onDestruction: if (root.watching) root.say("destroyed " + objectName) } }
    Component { id: obj; QtObject { property int n: 1; Component.onDestruction: if (root.watching) root.say("destroyed " + objectName) } }
    Component { id: rect; Rectangle { Item { } Component.onDestruction: if (root.watching) root.say("destroyed " + objectName) } }
    QtObject { id: plain }
    Timer { id: timer }
    function tryOne(name, c, p, props) {
        var o = props === undefined ? c.createObject(p) : c.createObject(p, props)
        o.objectName = name
        say(name + ": " + kind(o) + " parent " + kind(o.parent) + " visible " + o.visible + " " + counts())
        return o
    }
    Component.onCompleted: {
        say(counts())
        var a = tryOne("item(QtObject)", item, plain)
        tryOne("item(Timer)", item, timer)
        tryOne("item(root)", item, root)
        tryOne("item(null)", item, null)
        tryOne("rect(QtObject)", rect, plain)
        tryOne("obj(QtObject)", obj, plain)
        tryOne("obj(root)", obj, root)
        tryOne("obj(null)", obj, null)
        a.parent = root
        say("item(QtObject) given a parent: parent " + kind(a.parent) + " " + counts())
        var i = item.incubateObject(plain, {}, Qt.Synchronous)
        i.object.objectName = "incubated(QtObject)"
        say("incubated(QtObject): " + kind(i.object) + " parent " + kind(i.object.parent) + " " + counts())
        tryOne("item(root, parent null)", item, root, { n: 7, parent: null })
        tryOne("item(QtObject, parent root)", item, plain, { parent: root })
        tryOne("item(null, parent root)", item, null, { parent: root })
        plain.destroy()
        say("destroy() called: " + counts())
        Qt.callLater(function() {
            say("after the QtObject is destroyed: " + counts())
            timer.destroy()
            Qt.callLater(function() {
                say("after the Timer is destroyed: " + counts())
                root.watching = false
                Qt.quit()
            })
        })
    }
}
