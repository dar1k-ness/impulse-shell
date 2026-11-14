// What `onFound` of qml/modules/rusty/screenTranslator/ScreenTextOverlay.qml leaves when the
// colours it is given lack a key. `ti.color` (a Rectangle's, with a RESET) has a binding;
// `tiText.color` is an alias of a Text's colour, which takes no `undefined`. With no
// `background` (the `{"error": ...}` the colour job gives for an image it cannot read)
// `Qt.color(undefined)` throws before anything is written and the binding stays; with no
// `text` the first write has taken the binding off, the second throws and the text keeps its
// colour. The last part is the colours after what the binding follows has changed.
// qt: both keys: #99336699 #000000 -> ok -> #99102030 #f0e0d0 | an error: #99336699 #000000 -> Error: "undefined" is not a valid color name -> #99336699 #000000 | no text: #99336699 #000000 -> Error: Cannot assign [undefined] to QColor -> #99102030 #000000 | no background: #99336699 #000000 -> Error: "undefined" is not a valid color name -> #99336699 #000000 | then: #99102030 #f0e0d0, #99996633 #000000, #99102030 #000000, #99996633 #000000
import QtQuick
Item {
    id: root
    property string result
    property color secondary: "#336699"
    function transparentize(color, percentage = 1) {
        var c = Qt.color(color);
        return Qt.rgba(c.r, c.g, c.b, c.a * (1 - percentage));
    }
    component Squeezed: Item {
        property alias color: textWidget.color
        Text { id: textWidget }
    }
    component TextItem: Rectangle {
        id: ti
        property alias label: tiText
        color: root.transparentize(root.secondary, 0.4)
        Behavior on color { ColorAnimation { duration: 0 } }
        function found(colors) {
            ti.color = root.transparentize(colors.background, 0.4);
            tiText.color = colors.text;
        }
        Squeezed {
            id: tiText
            Behavior on color { ColorAnimation { duration: 0 } }
        }
    }
    TextItem { id: a }
    TextItem { id: b }
    TextItem { id: c }
    TextItem { id: d }
    function run(label, t, json) {
        var s = label + ": " + t.color + " " + t.label.color + " -> ";
        try { t.found(JSON.parse(json)); s += "ok" } catch (e) { s += e }
        return s + " -> " + t.color + " " + t.label.color;
    }
    Component.onCompleted: {
        var l = [
            run("both keys", a, '{"background": "#102030", "text": "#f0e0d0"}'),
            run("an error", b, '{"error": "Could not decode image data"}'),
            run("no text", c, '{"background": "#102030"}'),
            run("no background", d, '{"text": "#f0e0d0"}')
        ];
        root.secondary = "#996633";
        l.push("then: " + [a, b, c, d].map(t => t.color + " " + t.label.color).join(", "));
        result = l.join(" | ");
    }
}
