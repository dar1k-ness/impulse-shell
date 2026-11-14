import QtQuick

// incubateObject of a Ready component with an incubation controller (a window's, as in a
// shell): the object is made on a later turn of the loop unless the mode is Qt.Synchronous.
Item {
    id: root
    function say(s) { console.log(s) }
    function st(s) {
        return s === Component.Null ? "Null" : s === Component.Ready ? "Ready"
             : s === Component.Loading ? "Loading" : s === Component.Error ? "Error" : String(s)
    }
    Component { id: inline; Item { property int n: 5; Component.onCompleted: root.say("item completed n " + n + " parent " + (parent === root ? "root" : parent)) } }
    Component { id: nested; Item { property int n: 6; Item { Component.onCompleted: root.say("nested child completed") } Component.onCompleted: root.say("nested completed n " + n) } }
    Component.onCompleted: {
        say("consts " + [Qt.Asynchronous, Qt.Synchronous, Component.PreferSynchronous, Component.Asynchronous, Component.Null, Component.Ready, Component.Loading, Component.Error])
        var a = inline.incubateObject(root, {n: 1})
        say("default: status " + st(a.status) + " object " + a.object)
        a.onStatusChanged = function(s) { say("default onStatusChanged " + st(s) + " n " + a.object.n + " same " + (s === a.status) + " this " + (this === a)) }
        var b = inline.incubateObject(root, {n: 2}, Qt.Asynchronous)
        say("async: status " + st(b.status) + " object " + b.object)
        b.onStatusChanged = function(s) { say("async onStatusChanged " + st(s) + " n " + b.object.n) }
        var c = inline.incubateObject(root, {n: 3}, Qt.Synchronous)
        say("sync: status " + st(c.status) + " n " + c.object.n)
        c.onStatusChanged = function(s) { say("sync onStatusChanged (never)") }
        var d = nested.incubateObject(root, {n: 4})
        say("forced before: status " + st(d.status))
        d.onStatusChanged = function(s) { say("forced onStatusChanged " + st(s)) }
        d.forceCompletion()
        say("forced after: status " + st(d.status) + " n " + d.object.n)
        d.forceCompletion()
        var e = inline.incubateObject(null, {n: 7})
        e.onStatusChanged = function(s) { say("null parent onStatusChanged " + st(s) + " parent " + e.object.parent) }
        var m = inline.incubateObject(root, {n: 8}, "x")
        say("mode 'x': status " + st(m.status))
        var g = inline.incubateObject(root)
        say("typeof " + typeof g + " " + g + " onStatusChanged " + typeof g.onStatusChanged + " forceCompletion " + typeof g.forceCompletion)
        g.onStatusChanged = 5
        say("onStatusChanged = 5 reads " + g.onStatusChanged)
        g.status = 7
        say("status written reads " + g.status)
        g.object = root
        say("object written reads " + g.object)
        g.foo = 1
        say("foo " + g.foo + " keys [" + Object.keys(g) + "] json " + JSON.stringify(g) + " own " + g.hasOwnProperty("status") + " in " + ("status" in g))
        try { Object.getPrototypeOf(g).status } catch (err) { say("status of the prototype: " + err) }
        // Components loading in the background: none has its document at the first read, and
        // all have it before the call asked for below.
        // qt: fresh: Loading Loading Loading same false progress 0
        // qt: incubate of loading: null
        // qt: missing: Loading progress 0
        // qt: sync of a loading file: Ready progress 1 the loading one: Loading
        // qt: loaded file asynchronously: Ready
        // qt: onCompleted ends
        // qt: fresh1 a progressChanged 1 Loading
        // qt: fresh1 a statusChanged Ready
        // qt: fresh1 b statusChanged Ready
        // qt: fresh2 statusChanged Ready
        // qt: missing progressChanged 1 Loading
        // qt: missing statusChanged Error
        // qt: missing again Error
        // qt: missing sync Error
        // qt: callLater
        var x1 = Qt.createComponent("Fresh1.qml", Component.Asynchronous)
        var x2 = Qt.createComponent("Fresh1.qml", Component.Asynchronous)
        var x3 = Qt.createComponent("Fresh2.qml", Component.Asynchronous)
        say("fresh: " + st(x1.status) + " " + st(x2.status) + " " + st(x3.status) + " same " + (x1 === x2) + " progress " + x1.progress)
        x1.statusChanged.connect(function() { say("fresh1 a statusChanged " + st(x1.status)) })
        x1.progressChanged.connect(function() { say("fresh1 a progressChanged " + x1.progress + " " + st(x1.status)) })
        x2.statusChanged.connect(function() { say("fresh1 b statusChanged " + st(x2.status)) })
        x3.statusChanged.connect(function() { say("fresh2 statusChanged " + st(x3.status)) })
        say("incubate of loading: " + x1.incubateObject(root))
        var m1 = Qt.createComponent("Missing.qml", Component.Asynchronous)
        say("missing: " + st(m1.status) + " progress " + m1.progress)
        m1.progressChanged.connect(function() { say("missing progressChanged " + m1.progress + " " + st(m1.status)) })
        m1.statusChanged.connect(function() {
            say("missing statusChanged " + st(m1.status))
            say("missing again " + st(Qt.createComponent("Missing.qml", Component.Asynchronous).status))
            say("missing sync " + st(Qt.createComponent("Missing.qml").status))
        })
        var s2 = Qt.createComponent("Fresh2.qml")
        s2.statusChanged.connect(function() { say("sync statusChanged (never)") })
        say("sync of a loading file: " + st(s2.status) + " progress " + s2.progress + " the loading one: " + st(x3.status))
        say("loaded file asynchronously: " + st(Qt.createComponent("Fresh2.qml", Component.Asynchronous).status))
        Qt.callLater(function() { say("callLater") })
        end.start()
        say("onCompleted ends")
    }
    Timer { id: end; interval: 100; onTriggered: { root.say("end children " + root.children.length); Qt.quit() } }
}
