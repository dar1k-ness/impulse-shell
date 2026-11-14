// A value type read from a field of a value type (`button.icon.color`, the color of a
// QQuickIcon) is a copy in Qt 6.12: it does not follow the field and a write to its own field
// does not reach it. `button.icon` itself is a reference to the property.
// qt: #00000000|#ff0000|#0000ff|#8000ff|#0000ff|7|#00000000|#00000000|true|true|#010203
import QtQuick
import QtQuick.Controls
Item {
    id: root
    property string result
    Button { id: b }
    Button { id: b2 }
    Component.onCompleted: {
        var l = []
        var c = b.icon.color; b.icon.color = "red"; l.push("" + c)
        var i = b.icon; var c2 = i.color; i.color = "blue"; l.push("" + c2, "" + b.icon.color)
        var c3 = b.icon.color; c3.r = 0.5; l.push("" + c3, "" + b.icon.color)
        var i2 = b2.icon; b2.icon.width = 7; l.push(i2.width)
        var c4 = b2.icon.color; b2.icon.color = "#123456"; var s = [c4]; b2.icon.color = undefined; l.push(s.join(","), "" + c4, c4 === c4, c4 == b2.icon.color)
        var t = b2.icon; t.color = "#010203"; var o = [t.color]; t.color = undefined; l.push(o.join(","))
        result = l.join("|")
    }
}
