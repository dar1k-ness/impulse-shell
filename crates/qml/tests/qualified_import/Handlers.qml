import QtQuick as QQ
import QtQuick.Controls as C
import QtQml as Q

QQ.Item {
    id: root
    width: 100
    height: 100
    property int count: 0
    property real smooth: 0
    property QQ.font f: Qt.font({"pixelSize": 9})
    property QQ.rect r: Qt.rect(1, 2, 3, 4)
    QQ.Behavior on smooth {
        enabled: false
        QQ.NumberAnimation {
            duration: 5
        }
    }
    QQ.SequentialAnimation on opacity {
        id: onOpacity
        running: false
        QQ.NumberAnimation {
            to: 0.5
            duration: 5
        }
    }
    QQ.TapHandler {
        id: tap
        onTapped: root.count++
    }
    QQ.HoverHandler {
        id: hover
    }
    QQ.DragHandler {
        id: drag
        target: null
    }
    QQ.WheelHandler {
        id: wheel
    }
    QQ.Shortcut {
        id: shortcut
        sequence: "Ctrl+K"
    }
    QQ.FontMetrics {
        id: metrics
        font.pixelSize: 20
    }
    QQ.TextMetrics {
        id: textMetrics
        text: "abc"
    }
    QQ.FontLoader {
        id: fonts
    }
    QQ.SystemPalette {
        id: palette
    }
    QQ.Rectangle {
        id: rect
        border.width: 2
        transform: [
            QQ.Rotation {
                angle: 45
            },
            QQ.Scale {
                xScale: 2
            },
            QQ.Translate {
                x: 5
            }
        ]
        layer.enabled: false
        QQ.Drag.active: false
        QQ.Drag.hotSpot.x: 3
    }
    QQ.DropArea {
        id: drop
    }
    QQ.Canvas {
        id: canvas
        width: 10
        height: 10
    }
    QQ.PathView {
        id: pathView
        model: 2
        delegate: QQ.Item {}
        path: QQ.Path {
            startX: 0
            startY: 0
            QQ.PathLine {
                x: 10
                y: 10
            }
        }
    }
    QQ.Column {
        id: column
        spacing: 1
        QQ.Repeater {
            id: rep
            model: QQ.ListModel {
                QQ.ListElement {
                    label: "x"
                }
                QQ.ListElement {
                    label: "y"
                }
            }
            delegate: QQ.Text {
                required property string label
                required property int index
                text: label + index
                height: 4
            }
        }
    }
    Q.Binding {
        id: binding
        target: root
        property: "objectName"
        value: "bound"
        when: root.count >= 0
    }
    Q.Timer {
        id: qmlTimer
        interval: 5
        running: true
        onTriggered: {
            console.log("QtQml timer", running, rep.count, rep.itemAt(1).text)
            Qt.quit()
        }
    }
    QQ.Component.onCompleted: {
        console.log("behavior", (smooth = 4, smooth), onOpacity.running, opacity)
        console.log("handlers", tap.enabled, tap.gesturePolicy, hover.hovered, drag.active, wheel.orientation, shortcut.sequence, shortcut.enabled)
        console.log("metrics", typeof metrics.height, typeof textMetrics.width, fonts.status === QQ.FontLoader.Null, typeof palette.window)
        console.log("transform", rect.transform.length, rect.transform[0].angle, rect.transform[1].xScale, rect.transform[2].x, rect.border.width)
        console.log("drag", rect.QQ.Drag.active, rect.QQ.Drag.hotSpot.x, drop.containsDrag, canvas.available, typeof canvas.getContext)
        console.log("path", pathView.path.startX, typeof pathView.path.pathElements)
        console.log("values", f.pixelSize, r.width, binding.property)
    }
}
