// cell 260x230 cols 3: closed closedLongText open
// qml/modules/common/widgets/StyledComboBox.qml:10 with its colours, fonts and animations as constants.
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Grid {
    columns: 3
    component Cell: Item { width: 260; height: 230 }
    component Combo: ComboBox {
        id: root
        property real buttonRadius: height / 2
        implicitHeight: 40
        Layout.fillWidth: true
        background: Rectangle {
            radius: root.buttonRadius
            color: "#405070"
        }
        indicator: Text {
            x: root.width - width - 16
            y: root.height / 2 - height / 2
            text: "v"
            font.pixelSize: 19
            color: "white"
            rotation: root.popup.visible ? 180 : 0
        }
        contentItem: Item {
            implicitWidth: buttonLayout.implicitWidth
            implicitHeight: buttonLayout.implicitHeight
            RowLayout {
                id: buttonLayout
                anchors.fill: parent
                spacing: 8
                anchors.leftMargin: 16
                anchors.rightMargin: 16
                Text {
                    Layout.fillWidth: true
                    Layout.alignment: Qt.AlignVCenter
                    color: "white"
                    text: root.displayText
                    elide: Text.ElideRight
                    verticalAlignment: Text.AlignVCenter
                }
            }
        }
        delegate: ItemDelegate {
            id: itemDelegate
            width: ListView.view ? ListView.view.width : root.width
            implicitHeight: 40
            required property var model
            required property int index
            property color color: root.currentIndex === itemDelegate.index ? "#405070" : "transparent"
            background: Rectangle {
                anchors.fill: parent
                radius: 12
                color: itemDelegate.color
            }
            contentItem: RowLayout {
                spacing: 8
                anchors.leftMargin: 12
                anchors.rightMargin: 12
                Text {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 19
                    color: "white"
                    text: itemDelegate.model[root.textRole]
                    elide: Text.ElideRight
                    verticalAlignment: Text.AlignVCenter
                }
            }
        }
        popup: Popup {
            y: root.height + 4
            width: root.width
            height: Math.min(listView.contentHeight + topPadding + bottomPadding, 300)
            padding: 8
            background: Item {
                Rectangle {
                    anchors.fill: parent
                    radius: 17
                    color: "#303030"
                }
            }
            contentItem: ListView {
                id: listView
                clip: true
                implicitHeight: contentHeight
                spacing: 2
                model: root.popup.visible ? root.delegateModel : null
                currentIndex: root.highlightedIndex
            }
        }
    }
    Cell {
        ColumnLayout {
            x: 10; y: 10; width: 240
            Combo { model: ["Option one", "Option two", "Option three"] }
        }
    }
    Cell {
        ColumnLayout {
            x: 10; y: 10; width: 240
            Combo { model: ["An option whose name does not fit in the button", "Option two"] }
        }
    }
    Cell {
        ColumnLayout {
            x: 10; y: 10; width: 240
            Combo { model: ["Option one", "Option two", "Option three"]; Component.onCompleted: popup.open() }
        }
    }
}
