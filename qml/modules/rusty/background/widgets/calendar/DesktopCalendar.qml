import QtQuick
import ri
import ri.services
import ri.modules.common
import ri.modules.common.widgets
import ri.modules.rusty.sidebarRight.calendar
import "../../../sidebarRight/calendar/calendar_layout.js" as CalendarLayout

/**
 * Desktop widget "calendar": the sidebar's month calendar on a card filling the box the
 * desktop container gives it, the month scaled to fit; without a size it takes the month's
 * own. A box the month would be too small to read in shows less of it instead: a wide low
 * one the current week, any other today's date, each drawn as large as the box has room for.
 * Hidden on the lock screen, like the original's background widgets.
 */
Item {
    id: root
    property real padding: 10
    readonly property real roomWidth: Math.max(1, root.width - root.padding * 2)
    readonly property real roomHeight: Math.max(1, root.height - root.padding * 2)
    // The smallest scale the day numbers are still read at.
    readonly property real legibleScale: 0.75
    readonly property real monthScale: Math.min(root.roomWidth / Math.max(1, calendar.width), root.roomHeight / Math.max(1, calendar.implicitHeight))
    readonly property real weekScale: Math.min(2, root.roomWidth / Math.max(1, week.implicitWidth), root.roomHeight / Math.max(1, week.implicitHeight))
    readonly property real dayScale: Math.min(4, root.roomWidth / Math.max(1, day.implicitWidth), root.roomHeight / Math.max(1, day.implicitHeight))
    readonly property string form: root.monthScale >= root.legibleScale ? "month" : root.weekScale >= root.legibleScale && root.roomWidth >= root.roomHeight * 1.6 ? "week" : "day"
    readonly property var today: DateTime.clock.date
    // The row of the month's layout today is in.
    readonly property var thisWeek: {
        const rows = CalendarLayout.getCalendarLayout(root.today, true);
        return rows.find(row => row.some(cell => cell.today === 1)) ?? rows[0];
    }
    implicitWidth: calendar.width + root.padding * 2
    implicitHeight: calendar.implicitHeight + root.padding * 2
    visible: opacity > 0
    opacity: GlobalStates.screenLocked ? 0 : 1
    Behavior on opacity {
        animation: Appearance.animation.elementMoveFast.numberAnimation.createObject(this)
    }

    StyledRectangularShadow {
        target: background
    }

    Rectangle {
        id: background
        anchors.fill: parent
        radius: Appearance.rounding.large
        color: Appearance.colors.colLayer1

        CalendarWidget {
            id: calendar
            visible: root.form === "month"
            anchors.centerIn: parent
            height: calendar.implicitHeight
            scale: root.monthScale
        }

        Column {
            id: week
            visible: root.form === "week"
            anchors.centerIn: parent
            spacing: 5
            scale: root.weekScale

            StyledText {
                anchors.horizontalCenter: parent.horizontalCenter
                text: root.today.toLocaleDateString(Qt.locale(), "MMMM yyyy")
                font.pixelSize: Appearance.font.pixelSize.normal
                font.weight: Font.DemiBold
                color: Appearance.colors.colOnLayer1
            }
            Row {
                spacing: 5
                Repeater {
                    model: CalendarLayout.weekDays
                    delegate: CalendarDayButton {
                        required property var modelData
                        day: Translation.tr(modelData.day)
                        isToday: modelData.today
                        bold: true
                        enabled: false
                    }
                }
            }
            Row {
                spacing: 5
                Repeater {
                    model: root.thisWeek
                    delegate: CalendarDayButton {
                        required property var modelData
                        day: modelData.day
                        isToday: modelData.today
                    }
                }
            }
        }

        Column {
            id: day
            visible: root.form === "day"
            anchors.centerIn: parent
            scale: root.dayScale

            StyledText {
                anchors.horizontalCenter: parent.horizontalCenter
                text: Qt.locale().toString(root.today, "dddd")
                font.pixelSize: Appearance.font.pixelSize.small
                font.weight: Font.DemiBold
                color: Appearance.colors.colPrimary
            }
            StyledText {
                anchors.horizontalCenter: parent.horizontalCenter
                text: Qt.locale().toString(root.today, "d")
                font.pixelSize: 56
                font.weight: Font.Medium
                color: Appearance.colors.colOnLayer1
            }
            StyledText {
                anchors.horizontalCenter: parent.horizontalCenter
                text: Qt.locale().toString(root.today, "MMMM")
                font.pixelSize: Appearance.font.pixelSize.small
                color: Appearance.colors.colSubtext
            }
        }
    }
}
