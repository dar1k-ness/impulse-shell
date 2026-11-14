import QtQuick

// Which arguments of createObject fit `createObject(QObject *, QVariantMap)`, what the other
// method says about the rest, and what each call returns.
Item {
    id: root
    Component { id: inline; Item { property int n: 5 } }
    Component.onCompleted: {
        var a0 = [["root", root], ["null", null], ["undef", undefined], ["1", 1], ["'x'", "x"], ["{}", {}], ["[]", []], ["true", true]]
        var a1 = [["-", 0], ["undef", undefined], ["null", null], ["{}", {}], ["{n:1}", {n: 1}], ["5", 5], ["'x'", "x"], ["[1]", [1]], ["fn", function() {}], ["Date", new Date(0)], ["/r/", /r/], ["true", true]]
        for (var i = 0; i < a0.length; i++) {
            for (var j = 0; j < a1.length; j++) {
                console.log("CALL " + a0[i][0] + ", " + a1[j][0])
                var r = j === 0 ? inline.createObject(a0[i][1]) : inline.createObject(a0[i][1], a1[j][1])
                console.log("  -> " + (r === null ? "null" : "n " + r.n + " parent " + (r.parent === root ? "root" : r.parent)))
            }
        }
        console.log("CALL (none)")
        inline.createObject()
        Qt.quit()
    }
}
