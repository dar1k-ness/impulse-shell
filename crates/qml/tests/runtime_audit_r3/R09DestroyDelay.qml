import QtQuick
Item {
    id: root
    property string log: ""
    function p(...a) { log += a.join(" ") + "\n"; }
    function t(name, f) { try { p(name, f()); } catch (e) { p(name, "THROW", e.message); } }
    Component { id: c; Item { property int v: 3; Component.onDestruction: root.p("destroyed", objectName) } }
    property var oa
    property var ob
    property var oz
    Component.onCompleted: {
        oa = c.createObject(root, { objectName: "a" });
        ob = c.createObject(root, { objectName: "b" });
        oz = c.createObject(root, { objectName: "z" });
        oa.destroy(300);
        ob.destroy(200); ob.destroy(400);
        oz.destroy();
        t("now", () => oa.v + " " + ob.v + " " + oz.v);
        t0.start(); t1.start(); t2.start(); t3.start();
    }
    Timer { id: t0; interval: 50; onTriggered: root.t("50ms", () => root.oa.v + " " + root.ob.v + " " + (root.oz === null)) }
    Timer { id: t1; interval: 250; onTriggered: root.t("250ms", () => root.oa.v + " " + (root.ob === null)) }
    Timer { id: t2; interval: 350; onTriggered: root.t("350ms", () => (root.oa === null) + " " + (root.ob === null)) }
    Timer { id: t3; interval: 450; onTriggered: root.t("450ms", () => (root.oa === null) + " " + (root.ob === null)) }
}
