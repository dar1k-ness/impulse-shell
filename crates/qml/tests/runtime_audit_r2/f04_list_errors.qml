import QtQuick
Item {
    id: r
    Item { id: a; Item { id: a1 } QtObject { id: q } }
    function t(f) { try { f(); console.log("ok", a.children.length, a.resources.length, a.data.length) } catch (x) { console.log(x.name, x.message) } }
    Component.onCompleted: {
        t(function() { a.children = undefined })
        t(function() { a.data = undefined })
        t(function() { a.resources = undefined })
        t(function() { a.visibleChildren = undefined })
        t(function() { a.visibleChildren = [] })
        t(function() { a.visibleChildren = a1 })
        t(function() { a.visibleChildren = null })
        t(function() { a.children = 5 })
        t(function() { a.children = "x" })
        t(function() { a.children = q })
        t(function() { a.resources = a1 })
        t(function() { a.children = null })
    }
}
