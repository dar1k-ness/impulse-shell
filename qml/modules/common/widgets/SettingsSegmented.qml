import QtQuick
import ri.modules.common
import ri.modules.common.widgets

/**
 * Segmented choice: a pill track with a highlight that slides to the selected option.
 * options: [{ label, value }]; values are compared with ===.
 */
Rectangle {
    id: root
    property var options: []
    property var currentValue
    property real segmentPadding: 14
    signal selected(var value)

    readonly property int currentIndex: {
        for (let i = 0; i < root.options.length; i++) {
            if (root.options[i].value === root.currentValue)
                return i;
        }
        return -1;
    }
    property real highlightX: 0
    property real highlightWidth: 0
    property bool highlightReady: false

    implicitHeight: 40
    implicitWidth: segmentRow.implicitWidth + 8
    radius: height / 2
    color: Appearance.colors.colLayer3

    Rectangle {
        id: highlight
        visible: root.currentIndex >= 0 && root.highlightWidth > 0
        x: root.highlightX
        y: 4
        width: root.highlightWidth
        height: root.height - 8
        radius: height / 2
        color: Appearance.colors.colPrimary
        Behavior on x {
            enabled: root.highlightReady
            NumberAnimation {
                duration: Appearance.animationCurves.expressiveFastSpatialDuration
                easing.type: Easing.BezierSpline
                easing.bezierCurve: Appearance.animationCurves.expressiveFastSpatial
            }
        }
        Behavior on width {
            enabled: root.highlightReady
            NumberAnimation {
                duration: Appearance.animationCurves.expressiveFastSpatialDuration
                easing.type: Easing.BezierSpline
                easing.bezierCurve: Appearance.animationCurves.expressiveFastSpatial
            }
        }
    }

    Row {
        id: segmentRow
        x: 4
        y: 4
        height: root.height - 8

        Repeater {
            model: root.options
            delegate: Item {
                id: segment
                required property var modelData
                required property int index
                readonly property bool current: index === root.currentIndex
                implicitWidth: segmentText.implicitWidth + root.segmentPadding * 2
                width: implicitWidth
                height: segmentRow.height

                function publish() {
                    if (!segment.current)
                        return;
                    root.highlightX = segmentRow.x + segment.x;
                    root.highlightWidth = segment.width;
                }
                onCurrentChanged: publish()
                onXChanged: publish()
                onWidthChanged: publish()
                Component.onCompleted: publish()

                StyledText {
                    id: segmentText
                    anchors.centerIn: parent
                    text: segment.modelData.label
                    font.pixelSize: Appearance.font.pixelSize.small
                    font.weight: Font.Medium
                    color: segment.current ? Appearance.colors.colOnPrimary : Appearance.colors.colOnLayer3
                    Behavior on color {
                        animation: Appearance.animation.elementMoveFast.colorAnimation.createObject(this)
                    }
                }
                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.selected(segment.modelData.value)
                }
            }
        }
    }

    // The highlight moves to the first selection at once: the slide is armed only after creation,
    // positioning and the bindings it set off are done
    Component.onCompleted: Qt.callLater(() => root.highlightReady = true)
}
