pragma ComponentBehavior: Bound

import ri.services
import ri.modules.common
import ri.modules.common.functions
import ri.modules.common.widgets
import QtQuick
import QtQuick.Layouts
import Rustshell

Item {
    id: root
    property real padding: 4
    readonly property var categories: [...HyprlandKeybinds.keybindCategories, ""]
    // As the periodic table next to it: a category at a time until fill(), whole after it.
    property bool whole: false
    property int made: 0
    function fill() {
        whole = true;
    }
    function toStart() {
        flickable.contentX = 0;
    }
    Timer {
        interval: 100
        repeat: true
        running: !root.whole && root.made < root.categories.length
        onTriggered: root.made += 1
    }
    implicitWidth: QsWindow?.window?.screen.width * 0.7 ?? 0
    implicitHeight: QsWindow?.window?.screen.height * 0.7 ?? 0

    StyledFlickable {
        id: flickable
        clip: true
        anchors.fill: parent
        anchors.margins: Appearance.rounding.small
        contentHeight: height
        contentWidth: flow.implicitWidth
        Flow {
            id: flow
            height: flickable.height
            flow: Flow.TopToBottom
            spacing: 10
            Repeater {
                model: ScriptModel {
                    values: root.whole ? root.categories : root.categories.slice(0, root.made)
                }
                delegate: CheatsheetKeybindsCategory {
                    required property var modelData
                    categoryName: modelData
                }
            }
        }
    }

    ScrollEdgeFade {
        target: flickable
        vertical: false
        color: Appearance.colors.colLayer0Base
    }
}
