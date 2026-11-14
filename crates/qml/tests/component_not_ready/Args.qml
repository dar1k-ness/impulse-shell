import QtQuick

// What createObject and incubateObject say about their arguments, on a declared Component,
// on one of Qt.createComponent, and on ones that are not Ready.
Item {
    id: root
    function say(s) { console.log(s) }
    Component { id: inline; Item { property int n: 5 } }
    function each(tag, c) {
        var calls = [
            ["(root, null)", function() { return c.createObject(root, null) }],
            ["(root, undefined)", function() { return c.createObject(root, undefined) }],
            ["(root, 5)", function() { return c.createObject(root, 5) }],
            ["(root, [1])", function() { return c.createObject(root, [1]) }],
            ["(root, 'x')", function() { return c.createObject(root, "x") }],
            ["(1, 5)", function() { return c.createObject(1, 5) }],
            ["(1)", function() { return c.createObject(1) }],
            ["({})", function() { return c.createObject({}) }],
            ["(null, {n: 1}, 3)", function() { return c.createObject(null, {n: 1}, 3) }],
            ["(undefined, {n: 1})", function() { return c.createObject(undefined, {n: 1}) }],
            ["(root, function)", function() { return c.createObject(root, function() {}) }],
            ["(root, new Date)", function() { return c.createObject(root, new Date(0)) }],
            ["incubate(root, 5)", function() { return c.incubateObject(root, 5, Qt.Synchronous) }],
            ["incubate(root, [1])", function() { return c.incubateObject(root, [1], Qt.Synchronous) }],
            ["incubate(1, {n: 2})", function() { return c.incubateObject(1, {n: 2}, Qt.Synchronous) }],
            ["incubate(root, {n: 2}, 'x')", function() { return c.incubateObject(root, {n: 2}, "x") }],
            ["incubate(root, {n: 2}, 7)", function() { return c.incubateObject(root, {n: 2}, 7) }],
            ["incubate(root, null, 1)", function() { return c.incubateObject(root, null, 1) }],
        ]
        for (var i = 0; i < calls.length; i++) {
            var r
            try { r = calls[i][1](); r = r === null ? "null" : r === undefined ? "undefined" : r.forceCompletion ? "incubator " + r.status + (r.forceCompletion(), " then " + r.status) + " n " + r.object.n : "object n " + r.n + " parent " + (r.parent === root ? "root" : r.parent) }
            catch (e) { r = "threw " + e }
            say(tag + " " + calls[i][0] + " -> " + r)
        }
    }
    Component.onCompleted: {
        each("ready", inline)
        each("file", Qt.createComponent("Good.qml"))
        each("error", Qt.createComponent("Missing.qml"))
        var loading = Qt.createComponent("Fresh1.qml", Component.Asynchronous)
        // qt: loading status 2 progress 0
        say("loading status " + loading.status + " progress " + loading.progress)
        each("loading", loading)
        Qt.quit()
    }
}
