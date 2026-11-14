import QtQuick
import Rustshell
import ri
import ri.services
import ri.modules.common
import ri.modules.common.widgets
import ri.modules.rusty.sidebarRight.todo

/**
 * Desktop widget "notes": the sidebar's to-do list as a sticky card filling the box the
 * desktop container gives it; without a size it is `cardHeight` tall, as in the sidebar.
 * A box too small for the list's tabs and its add button shows the unfinished tasks alone,
 * at the text size of the full list: their number, then each with a button that ticks it off.
 * Hidden on the lock screen, like the original's background widgets.
 */
Item {
    id: root
    // Setting (widgets.toml).
    property int cardHeight: 350
    property real padding: 10
    // The smallest size the list's two tabs, its placeholder and its add button fit in.
    readonly property real listMinWidth: 290
    readonly property real listMinHeight: 230
    readonly property real listWidth: Math.max(1, root.width - root.padding * 2)
    readonly property real listHeight: Math.max(1, root.height - root.padding * 2)
    readonly property bool compact: root.listWidth < root.listMinWidth || root.listHeight < root.listMinHeight
    readonly property var unfinished: Todo.list.map((item, i) => Object.assign({}, item, {
                "originalIndex": i
            })).filter(item => !item.done)
    implicitWidth: 320
    implicitHeight: root.cardHeight
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

        TodoWidget {
            visible: !root.compact
            x: root.padding
            y: root.padding
            width: root.listWidth
            height: root.listHeight
        }

        Item {
            id: shortList
            visible: root.compact
            x: root.padding
            y: root.padding
            width: root.listWidth
            height: root.listHeight

            Row {
                id: count
                spacing: 6

                MaterialSymbol {
                    anchors.verticalCenter: parent.verticalCenter
                    text: "checklist"
                    iconSize: Appearance.font.pixelSize.larger
                    color: Appearance.colors.colOnLayer1
                }
                StyledText {
                    anchors.verticalCenter: parent.verticalCenter
                    text: root.unfinished.length
                    font.pixelSize: Appearance.font.pixelSize.normal
                    font.weight: Font.DemiBold
                    color: Appearance.colors.colOnLayer1
                }
            }

            StyledListView {
                anchors.top: count.bottom
                anchors.topMargin: 6
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.bottom: parent.bottom
                clip: true
                spacing: 4
                animateAppearance: false
                model: ScriptModel {
                    values: root.unfinished
                }
                delegate: Rectangle {
                    id: task
                    required property var modelData
                    width: ListView.view.width
                    implicitHeight: Math.max(tick.implicitHeight, taskText.implicitHeight) + 8
                    color: Appearance.colors.colLayer2
                    radius: Appearance.rounding.small

                    TodoItemActionButton {
                        id: tick
                        anchors.left: parent.left
                        anchors.leftMargin: 4
                        anchors.verticalCenter: parent.verticalCenter
                        onClicked: Todo.markDone(task.modelData.originalIndex)
                        contentItem: MaterialSymbol {
                            anchors.centerIn: parent
                            horizontalAlignment: Text.AlignHCenter
                            text: "check"
                            iconSize: Appearance.font.pixelSize.larger
                            color: Appearance.colors.colOnLayer1
                        }
                    }
                    StyledText {
                        id: taskText
                        anchors.left: tick.right
                        anchors.leftMargin: 6
                        anchors.right: parent.right
                        anchors.rightMargin: 8
                        anchors.verticalCenter: parent.verticalCenter
                        text: task.modelData.content
                        wrapMode: Text.Wrap
                        maximumLineCount: 2
                        elide: Text.ElideRight
                    }
                }
            }

            Column {
                visible: root.unfinished.length === 0
                anchors.centerIn: parent
                spacing: 5

                MaterialSymbol {
                    anchors.horizontalCenter: parent.horizontalCenter
                    iconSize: 40
                    color: Appearance.m3colors.m3outline
                    text: "check_circle"
                }
                StyledText {
                    anchors.horizontalCenter: parent.horizontalCenter
                    color: Appearance.m3colors.m3outline
                    text: Translation.tr("Nothing here!")
                }
            }
        }
    }
}
