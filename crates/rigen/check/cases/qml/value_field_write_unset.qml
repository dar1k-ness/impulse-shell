// A write to a field of a value type property of a Qt type reaches the property, also one
// nothing set, whose value until then is what the type starts with: `clip.sourceClipRect.x =
// 4`. The property's signal comes once for a write that changes it and not for one that
// leaves it as it was; a property that cannot be written keeps its value; a write through an
// alias of the property does the same and wakes the alias once. Each step: the signals of
// sourceClipRect (unset, then set), offset, paddingRect, scale, fillTransform, canvasWindow,
// the aliased offset and its alias it sent, then what it gives.
// qt: 100000000,QRectF(4, 0, 0, 0);000000000;100000000,QRectF(4, 0, 7, 0);010000000,QRectF(9, 2, 3, 4);010000000,QRectF(9, 2, 3, 8);000000000;001000000,QVector2D(5, 0);000000000;001000000,QVector2D(5, 6);000100000,QRectF(0, 0, 2, 0);000000000;000010000,QSizeF(1, 3);000000000;000001000,2;000000000;000000100,QRectF(0, 3, 0, 0);000000000;000000000,QPointF(0, 0);000000000,QRectF(0, 0, 0, 0);100000000,QRectF(11, 0, 7, 0);001000000,QVector2D(5, 1);000000011,QVector2D(2, 0);000000000;000000011,QVector2D(2, 3);000000011,QVector2D(2, 4);
import QtQuick
import QtQuick.Effects
import QtQuick.Shapes
Item {
    id: root
    property string result: ""
    property var n: [0, 0, 0, 0, 0, 0, 0, 0, 0]
    Image { id: clip; onSourceClipRectChanged: root.n[0]++ }
    Image { id: clipped; sourceClipRect: Qt.rect(1, 2, 3, 4); onSourceClipRectChanged: root.n[1]++ }
    RectangularShadow { id: shadow; onOffsetChanged: root.n[2]++ }
    MultiEffect { id: effect; onPaddingRectChanged: root.n[3]++ }
    Shape {
        ShapePath { id: path; onScaleChanged: root.n[4]++; onFillTransformChanged: root.n[5]++ }
    }
    Canvas { id: canvas; onCanvasWindowChanged: root.n[6]++ }
    Rectangle { id: rect }
    RectangularShadow { id: aliased; onOffsetChanged: root.n[7]++ }
    property alias off: aliased.offset
    onOffChanged: n[8]++
    function step(f) {
        var was = n.slice()
        var r = f()
        result += n.map((c, i) => c - was[i]).join("") + (r === undefined ? "" : "," + r) + ";"
    }
    Component.onCompleted: {
        step(() => { clip.sourceClipRect.x = 4; return clip.sourceClipRect })
        step(() => { clip.sourceClipRect.x = 4 })
        step(() => { clip.sourceClipRect.width = 7; return clip.sourceClipRect })
        step(() => { clipped.sourceClipRect.x = 9; return clipped.sourceClipRect })
        step(() => { clipped.sourceClipRect.height += 4; return clipped.sourceClipRect })
        step(() => { clipped.sourceClipRect.height = 8 })
        step(() => { shadow.offset.x = 5; return shadow.offset })
        step(() => { shadow.offset.y = 0 })
        step(() => { shadow.offset.y = 6; return shadow.offset })
        step(() => { effect.paddingRect.width = 2; return effect.paddingRect })
        step(() => { effect.paddingRect.width = 2 })
        step(() => { path.scale.height = 3; return path.scale })
        step(() => { path.scale.width = 1 })
        step(() => { path.fillTransform.m11 = 2; return path.fillTransform.m11 })
        step(() => { path.fillTransform.m11 = 2 })
        step(() => { canvas.canvasWindow.y = 3; return canvas.canvasWindow })
        step(() => { canvas.canvasWindow.y = 3 })
        step(() => { rect.transformOriginPoint.x = 5; return rect.transformOriginPoint })
        step(() => { rect.childrenRect.x = 5; return rect.childrenRect })
        step(() => { var f = "x"; clip["sourceClipRect"][f] = 11; return clip.sourceClipRect })
        step(() => { shadow.offset.y -= 5; return shadow.offset })
        step(() => { root.off.x = 2; return aliased.offset })
        step(() => { root.off.x = 2 })
        step(() => { off.y = 3; return off })
        step(() => { aliased.offset.y = 4; return off })
    }
}
