// The binding table of an object: values, signal handlers, attached properties, group
// properties, values written through an alias. A handler hears the writes that come after it
// in the table, wherever they stand in the file.
// qt: mirrored font base.b=6 inner2 base.a=5 inner base.e=50 base.done
import QtQuick
import QtQuick.Controls
Button {
    id: root
    property string result: Log.s
    property alias inner: sub.a
    property alias inner2: sub.b
    inner: 5
    inner2: 6
    font.pixelSize: 20
    LayoutMirroring.enabled: true
    width: 10
    onWidthChanged: Log.add("width")
    onMirroredChanged: Log.add("mirrored")
    onFontChanged: Log.add("font")
    onInnerChanged: Log.add("inner")
    onInner2Changed: Log.add("inner2")
    Base { id: sub }
}
