// The row of choices of the settings (qml/modules/common/widgets/SettingsSegmented.qml): a
// choice is as wide as its label, the mark lies under the chosen one.
// Qt 6.12: cd crates/shell/tests/segmented_width && QT_FORCE_STDERR_LOGGING=1 QT_QPA_PLATFORM=offscreen qml seg.qml
import QtQuick

Item {
    id: page
    width: 400
    height: 60
    property string log: ""

    // x, width, implicitWidth of every choice; the row, the track, the mark.
    function sizes() {
        let s = "";
        for (let i = 0; i < segmentRow.children.length; i++) {
            const c = segmentRow.children[i];
            if (c.modelData === undefined)
                continue;
            s += c.x + "," + c.width + "," + c.implicitWidth + " ";
        }
        return s + "| row " + segmentRow.implicitWidth + " track " + root.implicitWidth
            + " mark " + root.highlightX + "," + root.highlightWidth;
    }

    function choose(value) {
        root.currentValue = value;
    }

    Rectangle {
        id: root
        property var options: [
            { label: "Hug", value: 0 },
            { label: "Float", value: 1 },
            { label: "Rect", value: 2 }
        ]
        property var currentValue: 1
        property real segmentPadding: 14
        readonly property int currentIndex: {
            for (let i = 0; i < root.options.length; i++) {
                if (root.options[i].value === root.currentValue)
                    return i;
            }
            return -1;
        }
        property real highlightX: 0
        property real highlightWidth: 0
        implicitHeight: 40
        implicitWidth: segmentRow.implicitWidth + 8

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

                    function publish(from) {
                        if (!segment.current)
                            return;
                        page.log += from + segment.x + "," + segment.width + " ";
                        root.highlightX = segmentRow.x + segment.x;
                        root.highlightWidth = segment.width;
                    }
                    onCurrentChanged: publish("c")
                    onXChanged: publish("x")
                    onWidthChanged: publish("w")
                    Component.onCompleted: publish("d")

                    Label {
                        id: segmentText
                        anchors.centerIn: parent
                        text: segment.modelData.label
                        font.pixelSize: 15
                        font.weight: Font.Medium
                        color: segment.current ? "red" : "blue"
                    }
                }
            }
        }
    }

    Timer {
        interval: 50
        running: true
        onTriggered: {
            console.log(page.sizes());
            console.log(page.log);
            page.choose(2);
            console.log(page.sizes());
            console.log(page.log);
            Qt.quit();
        }
    }
}
