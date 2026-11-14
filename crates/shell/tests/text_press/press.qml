// A press on a TextField inside a Button's `contentItem`, the Button having a MouseArea child
// that fills it. A `contentItem` with an id inside is made before the MouseArea, which then
// lies above it and takes the press; one without an id is deferred, lies above the MouseArea,
// and its TextField takes the press and the focus (QQuickTextInput::mousePressEvent).
// Clicks at (50, 30) and (50, 130); the answer of Qt 6.12 is taken with `qt_press.py`.
// qt: 1 1 false | 0 0 true
import QtQuick
import QtQuick.Controls
Item {
    id: root
    width: 300; height: 200
    property string result: ""
    property int presses: 0
    property int clicks: 0
    property int presses2: 0
    property int clicks2: 0
    property var plain: noId.contentItem.children[0]
    Button {
        id: withId
        width: 300; height: 100
        MouseArea {
            anchors.fill: parent
            onPressed: root.presses++
            onClicked: root.clicks++
        }
        contentItem: Item {
            TextField {
                id: f; x: 10; y: 10; width: 200; height: 40
                // MaterialTextField's cursor-shape MouseArea, above the field.
                MouseArea { anchors.fill: parent; acceptedButtons: Qt.NoButton; hoverEnabled: true }
            }
        }
    }
    Button {
        id: noId
        y: 100; width: 300; height: 100
        MouseArea {
            anchors.fill: parent
            onPressed: root.presses2++
            onClicked: root.clicks2++
        }
        contentItem: Item {
            TextField {
                x: 10; y: 10; width: 200; height: 40
                MouseArea { anchors.fill: parent; acceptedButtons: Qt.NoButton; hoverEnabled: true }
            }
        }
    }
    function done() {
        result = presses + " " + clicks + " " + f.activeFocus + " | " + presses2 + " " + clicks2 + " " + plain.activeFocus
    }
}
