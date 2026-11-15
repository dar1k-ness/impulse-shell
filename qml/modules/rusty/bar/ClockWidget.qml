import ri.modules.common
import ri.modules.common.widgets
import ri.services
import QtQuick
import QtQuick.Layouts

Item {
    id: root
    property bool borderless: Config.options.bar.borderless
    property bool showDate: Config.options.bar.verbose
    // The length the bar fixes it at, -1 when it takes its own: the date goes when it has no
    // room there, then the time gives way to a clock icon.
    property real room: -1
    readonly property bool fitsTime: room < 0 || room >= time.implicitWidth
    readonly property bool fitsDate: room < 0 || room >= time.implicitWidth + dot.implicitWidth + date.implicitWidth + rowLayout.spacing * 2
    implicitWidth: rowLayout.implicitWidth
    implicitHeight: Appearance.sizes.barHeight

    RowLayout {
        id: rowLayout
        anchors.centerIn: parent
        spacing: 4

        MaterialSymbol {
            visible: !root.fitsTime
            text: "schedule"
            iconSize: Appearance.font.pixelSize.large
            color: Appearance.colors.colOnLayer1
        }

        StyledText {
            id: time
            visible: root.fitsTime
            font.pixelSize: Appearance.font.pixelSize.large
            color: Appearance.colors.colOnLayer1
            text: DateTime.time
        }

        StyledText {
            id: dot
            visible: root.showDate && root.fitsDate
            font.pixelSize: Appearance.font.pixelSize.small
            color: Appearance.colors.colOnLayer1
            text: "•"
        }

        StyledText {
            id: date
            visible: root.showDate && root.fitsDate
            font.pixelSize: Appearance.font.pixelSize.small
            color: Appearance.colors.colOnLayer1
            text: DateTime.longDate
        }
    }

    MouseArea {
        id: mouseArea
        anchors.fill: parent
        hoverEnabled: !Config.options.bar.tooltips.clickToShow

        ClockWidgetPopup {
            hoverTarget: mouseArea
        }
    }
}
