import QtQuick

// What a component that is not Ready answers: Error (no such file, a syntax error, a type
// that does not exist) and Loading (Component.Asynchronous, until the loop turns).
Item {
    id: root
    function say(s) { console.log(s) }
    function st(c) {
        return c.status === Component.Null ? "Null" : c.status === Component.Ready ? "Ready"
             : c.status === Component.Loading ? "Loading" : "Error"
    }
    function tryAll(tag, c) {
        say(tag + " status " + st(c) + " progress " + c.progress)
        var o = c.createObject(root)
        say(tag + " createObject(root) " + o + " " + (o === null) + " " + typeof o)
        say(tag + " createObject() " + c.createObject())
        say(tag + " createObject(null, props) " + c.createObject(null, {n: 1}))
        say(tag + " createObject(root, unknown) " + c.createObject(root, {nope: 1}))
        say(tag + " createObject(1) " + c.createObject(1))
        say(tag + " createObject(root, 5) " + c.createObject(root, 5))
        o = c.incubateObject(root)
        say(tag + " incubateObject(root) " + o + " " + (o === null) + " " + typeof o)
        say(tag + " incubateObject() " + c.incubateObject())
        say(tag + " incubateObject(root, props, sync) " + c.incubateObject(root, {n: 2}, Qt.Synchronous))
        say(tag + " incubateObject(root, props, async) " + c.incubateObject(root, {n: 2}, Qt.Asynchronous))
        say(tag + " incubateObject(1, props) " + c.incubateObject(1, {n: 2}))
        say(tag + " incubateObject(root, 5) " + c.incubateObject(root, 5))
        say(tag + " incubateObject(root, undefined) " + c.incubateObject(root, undefined))
        say(tag + " children " + root.children.length)
    }
    Component.onCompleted: {
        var missing = Qt.createComponent("Missing.qml")
        say("missing errorString " + JSON.stringify(missing.errorString().split("/").pop()))
        tryAll("missing", missing)
        tryAll("broken", Qt.createComponent("Broken.qml"))
        var badType = Qt.createComponent("BadType.qml")
        say("badType errorString " + JSON.stringify(badType.errorString().split("/").pop()))
        tryAll("badType", badType)
        var loading = Qt.createComponent("Good.qml", Component.Asynchronous)
        loading.statusChanged.connect(function() {
            say("loading statusChanged " + st(loading) + " progress " + loading.progress)
            var o = loading.createObject(root)
            say("loading then createObject " + (o ? o.n : o))
            var inc = loading.incubateObject(root, {n: 3})
            say("loading then incubateObject status " + inc.status + " " + inc.object)
            inc.onStatusChanged = function(s) {
                say("loading then incubated " + s + " n " + inc.object.n + " children " + root.children.length)
                Qt.quit()
            }
        })
        // qt: loading status Loading progress 0
        tryAll("loading", loading)
        // Qt's loader thread had the document before the first read: no statusChanged comes.
        if (loading.status !== Component.Loading)
            Qt.quit()
    }
}
