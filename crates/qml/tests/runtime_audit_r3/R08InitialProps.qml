import QtQuick
Item {
    id: root
    property string log: ""
    function p(...a) { log += a.join(" ") + "\n"; }
    function t(name, f) { try { p(name, f()); } catch (e) { p(name, "THROW", e.message); } }
    Component { id: c; QtObject { property int known: 1 } }
    Component { id: ci; Item { property int known: 1 } }
    Component.onCompleted: {
        t("unknown", () => { var o = c.createObject(root, { nope: 5, known: 7 }); return o.known + " " + o.nope + " " + ("nope" in o) + " " + Object.keys(o).indexOf("nope"); });
        t("builtin", () => { var o = ci.createObject(root, { width: 9, nope: 5, objectName: "n" }); return o.width + " " + o.objectName + " " + o.nope + " " + o.known; });
        t("null-parent", () => { var o = c.createObject(null, { other: "x" }); return o.other + " " + o.hasOwnProperty("other"); });
    }
}
