// A `property` (an alias too) declared under the name of a FINAL C++ property is a load error,
// whatever its type: `visible`, `width`, `height`, `x` of an Item, `width` of a Text,
// `text` of a TextMetrics, `topLeftRadius` of a Rectangle. A property that is not FINAL may be
// declared again (`enabled`, `clip`, Text's `text`, FontMetrics' `height`, Timer's `interval`,
// `radius`), so may a signal or a function of a FINAL property's name, and a property of a QML
// base type in a type derived from it.
// qt: derived x dx; 0 3:5: Cannot override FINAL property; 1 3:5: Cannot override FINAL property; 2 3:5: Cannot override FINAL property; 3 3:5: Cannot override FINAL property; 4 3:5: Cannot override FINAL property; 5 3:5: Cannot override FINAL property; 6 ok 5; 7 ok 5; 8 3:5: Cannot override FINAL property; 9 ok 5; 10 3:5: Cannot override FINAL property; 11 ok 5; 12 ok 5; 13 3:5: Cannot override FINAL property; 14 ok 5; 15 ok 1; 16 ok 1
import QtQuick
Item {
    id: root
    property string result: ""
    component Base: Item {
        property int own: 1
    }
    Base {
        id: derived
        property string own: "x"
        property string seen: "d" + own
    }
    function load(body) {
        try {
            var o = Qt.createQmlObject("import QtQuick\n" + body, root, "inline.qml")
            var r = "ok " + o.p
            o.destroy()
            return r
        } catch (e) {
            var m = /:(\d+:\d+: .*)$/.exec(String(e.message).trim())
            return m ? m[1] : String(e.message)
        }
    }
    Component.onCompleted: {
        var cases = [
            "Item {\n    property int visible: 5\n    property var p: visible\n}",
            "Item {\n    property list<var> visible: [1]\n    property var p: visible.length\n}",
            "Item {\n    property int width: 5\n    property var p: width\n}",
            "Item {\n    property list<var> width: [1]\n    property var p: width.length\n}",
            "Item {\n    readonly property int height: 5\n    property var p: height\n}",
            "Item {\n    property alias x: inner.y\n    property var p: x\n    Item { id: inner; y: 3 }\n}",
            "Item {\n    property int enabled: 5\n    property var p: enabled\n}",
            "Item {\n    property int clip: 5\n    property var p: clip\n}",
            "Text {\n    property int width: 5\n    property var p: width\n}",
            "Text {\n    property int text: 5\n    property var p: text\n}",
            "TextMetrics {\n    property int text: 5\n    property var p: text\n}",
            "FontMetrics {\n    property int height: 5\n    property var p: height\n}",
            "Timer {\n    property int interval: 5\n    property var p: interval\n}",
            "Rectangle {\n    property int topLeftRadius: 5\n    property var p: topLeftRadius\n}",
            "Rectangle {\n    property int radius: 5\n    property var p: radius\n}",
            "Item {\n    signal visibleChanged()\n    property var p: 1\n}",
            "Item {\n    function visible() { return 2 }\n    property var p: 1\n}",
        ]
        var out = []
        for (var i = 0; i < cases.length; i++)
            out.push(i + " " + load(cases[i]))
        result = "derived " + derived.own + " " + derived.seen + "; " + out.join("; ")
    }
}
