// An object or value a Qt type always has reads, where nothing sets it, what Qt gives a fresh
// instance: the `drag` of a MouseArea, the `visibleArea` and content size of a Flickable, the
// `section` of a ListView, the point of a handler, the axis of a DragHandler, a font, an
// easing curve and an icon as text, the colours of the Basic style's text field.
// qt: 3 true 10 null 0 0 -1 -1 1  -1 QPointF(0, 0) QSizeF(-1, -1) core pointer QPointingDeviceUniqueId(-1) 1.7976931348623157e+308 true QFont(Sans Serif,9,-1,5,400,0,0,0,0,0,0,0,0,0,0,1,,0,0) QFont(Sans Serif,15,-1,5,700,0,0,0,0,0,0,0,0,0,0,1,,0,0) [object V4ReferenceObject] 12 false QEasingCurve(Linear, 1, 1.70158, 0.3, ) 0 QQuickIcon(, , 24, 24, #00000000, true) #353637 #0066ff #88353637 QSizeF(1, 1) 1 2 QSize(0, 0) QPointF(20, 10) QPointF(40, 20) 1 object * 0 true
import QtQuick
import QtQuick.Controls
import QtQuick.Shapes
Item {
    id: root
    MouseArea { id: area }
    Flickable { id: flick }
    ListView { id: list }
    Item { TapHandler { id: tap } DragHandler { id: dragger } }
    Text { id: text }
    Text { id: big; font.pointSize: 15; font.bold: true }
    NumberAnimation { id: anim }
    Button { id: button }
    TextField { id: field }
    Shape { ShapePath { id: path } }
    Image { id: image }
    Rectangle { id: rect; width: 40; height: 20 }
    Rectangle { id: corner; width: 40; height: 20; transformOrigin: Item.BottomRight }
    StackView { id: stack }
    property string result
    Component.onCompleted: result = [area.drag.axis, area.drag.smoothed, area.drag.threshold,
        area.drag.target, flick.visibleArea.widthRatio, flick.visibleArea.yPosition,
        flick.contentWidth, flick.contentHeight, list.section.labelPositioning,
        list.section.property, tap.point.id, tap.point.position, tap.point.ellipseDiameters,
        tap.point.device.name, tap.point.uniqueId, dragger.xAxis.maximum, dragger.yAxis.enabled,
        text.font, big.font, text.font.features, text.fontInfo.pixelSize, text.fontInfo.bold,
        anim.easing, anim.easing.bezierCurve.length, button.icon, field.color,
        field.selectionColor, field.placeholderTextColor, path.scale, path.trim.end,
        path.dashPattern.length, image.sourceSize, rect.transformOriginPoint,
        corner.transformOriginPoint, text.effectiveHorizontalAlignment, typeof rect.palette,
        stack.pushEnter.from, image.colorSpace.gamma, flick.contentItem.objectName === ""
    ].map(String).join(" ")
}
