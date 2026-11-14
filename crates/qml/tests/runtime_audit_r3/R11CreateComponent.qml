import QtQuick
import "sub"
Item {
    id: root
    property string log: ""
    function p(...a) { log += a.join(" ") + "\n"; }
    function t(name, f) { try { p(name, f()); } catch (e) { p(name, "THROW", e.message); } }
    Maker { id: maker }
    Component.onCompleted: {
        t("relative to the caller", () => maker.make());
        t("relative to the caller, parent elsewhere", () => maker.makeIn(root));
        t("own document", () => Qt.createComponent("Leaf.qml").createObject(null).who);
        t("url", () => { var k = Qt.createComponent("Leaf.qml"); return typeof k.url + " " + (k.url.toString() === Qt.resolvedUrl("Leaf.qml").toString()) + " " + /\/Leaf\.qml$/.test("" + k.url) + " " + k.status; });
        t("url of sub", () => /\/sub\/Leaf\.qml$/.test("" + maker.url()));
    }
}
