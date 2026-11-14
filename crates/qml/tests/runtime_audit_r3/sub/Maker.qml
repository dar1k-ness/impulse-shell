import QtQuick
QtObject {
    function make() {
        var c = Qt.createComponent("Leaf.qml");
        if (c.status !== Component.Ready) return "status " + c.status + " " + c.errorString();
        return c.createObject(null).who;
    }
    function makeIn(parent) {
        var c = Qt.createComponent("Leaf.qml", parent);
        return c.createObject(parent).who;
    }
    function url() { return Qt.createComponent("Leaf.qml").url; }
}
