import QtQuick
Item {
    id: root
    property QtObject q1: QtObject { property int a: 1 }
    property QtObject q2: QtObject { property int a: 1 }
    property QtObject q3: QtObject { property int a: 1 }
    property QtObject q4: QtObject { property int a: 1 }
    function t(name, f) { var s; try { s = f() } catch (e) { s = "THROW " + e.name + ": " + e.message } console.log(name + " => " + s) }
    function st(q) { return Object.isFrozen(q) + "," + Object.isSealed(q) + "," + Object.isExtensible(q) }
    function wr(q) { var r; try { q.a = q.a + 1; r = "a=" + q.a } catch (e) { r = e.name + ": " + e.message } try { q.b1n = 1; r += " new=" + q.b1n } catch (e) { r += " " + e.name + ": " + e.message } return r }
    Component.onCompleted: {
        t("Z0", function() { return st(q4) + " " + wr(q4) })
        t("Z1", function() { return (Object.freeze(q1) === q1) + " " + st(q1) + " " + wr(q1) })
        t("Z2", function() { return (Object.seal(q2) === q2) + " " + st(q2) + " " + wr(q2) })
        t("Z3", function() { return (Object.preventExtensions(q3) === q3) + " " + st(q3) + " " + wr(q3) })
        t("Z4", function() { return Reflect.preventExtensions(q4) + " " + st(q4) + " " + wr(q4) })
        t("Z5", function() { return (Object.freeze(root) === root) + " " + st(root) })
        t("Z6", function() { root.width = 5; return root.width })
        Qt.quit()
    }
}
