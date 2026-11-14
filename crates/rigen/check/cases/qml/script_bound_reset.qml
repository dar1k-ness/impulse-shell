// A property of a Qt type written from a script: a value, `undefined`, a `Qt.binding` that
// gives a value, `undefined` and the value again, `undefined`. A value makes the property a
// set one, `undefined` is its RESET: a side margin follows `margins` again, a corner the
// `radius`, a field of `icon` has what QQuickIcon gives a new one, not what the style set.
// qt: anchors.leftMargin 0,3,0,3,0,3,0 | anchors.leftMargin 11,3,11,3,11,3,11 | Layout.leftMargin 11,3,11,3,11,3,11 | leftMargin 11,3,11,3,11,3,11 | leftPadding 11,3,11,3,11,3,11 | anchors.rightMargin 0,3,0,3,0,3,0 | anchors.rightMargin 11,3,11,3,11,3,11 | Layout.rightMargin 11,3,11,3,11,3,11 | rightMargin 11,3,11,3,11,3,11 | rightPadding 11,3,11,3,11,3,11 | anchors.topMargin 0,3,0,3,0,3,0 | anchors.topMargin 11,3,11,3,11,3,11 | Layout.topMargin 11,3,11,3,11,3,11 | topMargin 11,3,11,3,11,3,11 | topPadding 11,3,11,3,11,3,11 | anchors.bottomMargin 0,3,0,3,0,3,0 | anchors.bottomMargin 11,3,11,3,11,3,11 | Layout.bottomMargin 11,3,11,3,11,3,11 | bottomMargin 11,3,11,3,11,3,11 | bottomPadding 11,3,11,3,11,3,11 | topLeftRadius 0,3,0,3,0,3,0 | topLeftRadius 7,3,7,3,7,3,7 | topRightRadius 0,3,0,3,0,3,0 | topRightRadius 7,3,7,3,7,3,7 | bottomLeftRadius 0,3,0,3,0,3,0 | bottomLeftRadius 7,3,7,3,7,3,7 | bottomRightRadius 0,3,0,3,0,3,0 | bottomRightRadius 7,3,7,3,7,3,7 | icon.width 24,27,0,27,0,27,0 | icon.height 24,27,0,27,0,27,0 | icon.color #00000000,#123456,#00000000,#123456,#00000000,#123456,#00000000 | icon.cache true,false,true,false,true,false,true | icon.name ,x,,x,,x,
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
Item {
    id: root
    property bool flag: true
    Item { id: plain }
    Item { id: wide; anchors.margins: 11 }
    RowLayout { Item { id: laid; Layout.margins: 11 } }
    Rectangle { id: rect }
    Rectangle { id: round; radius: 7 }
    Button { id: button }
    Popup { id: popup; margins: 11 }
    Control { id: control; padding: 11 }
    property string result
    function probe(o, path, v) {
        var ks = path.split("."), last = ks.pop()
        var t = o
        for (var k of ks) t = t[k]
        var out = [t[last]]
        t[last] = v; out.push(t[last])
        t[last] = undefined; out.push(t[last])
        root.flag = true
        t[last] = Qt.binding(() => root.flag ? v : undefined); out.push(t[last])
        root.flag = false; out.push(t[last])
        root.flag = true; out.push(t[last])
        t[last] = undefined; out.push(t[last])
        return path + " " + out.join(",")
    }
    Component.onCompleted: {
        var l = []
        for (var s of ["left", "right", "top", "bottom"]) {
            l.push(probe(plain, "anchors." + s + "Margin", 3))
            l.push(probe(wide, "anchors." + s + "Margin", 3))
            l.push(probe(laid, "Layout." + s + "Margin", 3))
            l.push(probe(popup, s + "Margin", 3))
            l.push(probe(control, s + "Padding", 3))
        }
        for (var c of ["topLeft", "topRight", "bottomLeft", "bottomRight"]) {
            l.push(probe(rect, c + "Radius", 3))
            l.push(probe(round, c + "Radius", 3))
        }
        l.push(probe(button, "icon.width", 27))
        l.push(probe(button, "icon.height", 27))
        l.push(probe(button, "icon.color", "#123456"))
        l.push(probe(button, "icon.cache", false))
        l.push(probe(button, "icon.name", "x"))
        result = l.join(" | ")
    }
}
