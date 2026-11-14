import ri.services
import ri.modules.common
import ri.modules.common.widgets
import QtQuick
import QtQuick.Layouts
import Rustshell
import Rustshell.Bluetooth

import ri.modules.rusty.sidebarRight.quickToggles.androidStyle

AbstractQuickPanel {
    id: root
    property bool editMode: false
    Layout.fillWidth: true

    // Sizes
    implicitHeight: (unusedShown ? contentItem.implicitHeight : usedRows.implicitHeight) + root.padding * 2
    Behavior on implicitHeight {
        animation: Appearance.animation.elementMove.numberAnimation.createObject(this)
    }

    // Edit mode makes the unused toggles first and shows them at the next frame: making them
    // takes longer than a frame, and motion that began before it would be over, unseen, by
    // the time the next frame is drawn.
    property bool unusedShown: false
    onEditModeChanged: {
        root.movedFrom = null;
        if (!root.editMode) root.unusedShown = false;
        else if (unused.item) unusedMade.restart(); // still there, fading out
    }
    Timer {
        id: unusedMade
        interval: 1
        onTriggered: root.unusedShown = root.editMode
    }

    // Motion of the toggles in edit mode. Before a button changes the toggles every button
    // writes where it is drawn to `movedFrom`, under its type. In the layout that follows, a
    // button that is placed elsewhere, or made anew in another row, starts from there and
    // slides to its place (AndroidQuickToggleButton.qml). `movedFrom` is cleared at the frame
    // after that layout: what moves from then on is moved by a width that is animating.
    property var movedFrom: null
    signal aboutToMove()
    function beforeMove() {
        root.movedFrom = {};
        root.aboutToMove();
        moved.interval = 1000;
        moved.restart();
    }
    function placed() {
        if (moved.interval === 1) return;
        moved.interval = 1;
        moved.restart();
    }
    Timer {
        id: moved
        onTriggered: root.movedFrom = null
    }
    property real spacing: 6
    property real padding: 6
    readonly property real baseCellWidth: {
        // This is the wrong calculation, but it looks correct in reality???
        // (theoretically spacing should be multiplied by 1 column less)
        const availableWidth = root.width - (root.padding * 2) - (root.spacing * (root.columns))
        return availableWidth / root.columns
    }
    readonly property real baseCellHeight: 56

    // Toggles
    readonly property list<string> availableToggleTypes: ["network", "bluetooth", "idleInhibitor", "easyEffects", "nightLight", "darkMode", "cloudflareWarp", "gameMode", "screenSnip", "colorPicker", "onScreenKeyboard", "mic", "audio", "notifications", "powerProfile","musicRecognition", "antiFlashbang"]
    readonly property int columns: Config.options.sidebar.quickToggles.android.columns
    readonly property list<var> toggles: Config.ready ? Config.options.sidebar.quickToggles.android.toggles : []
    readonly property list<var> toggleRows: toggleRowsForList(toggles)
    // One entry for each type, the same one every time: a row of the unused toggles is then
    // the row it was, and the toggle it shows stays the one a click on it adds.
    readonly property var unusedEntries: availableToggleTypes.map(type => { return { type: type, size: 1 } })
    readonly property list<var> unusedToggles: unusedEntries.filter(entry => !toggles.some(toggle => (toggle && toggle.type === entry.type)))
    readonly property list<var> unusedToggleRows: toggleRowsForList(unusedToggles)

    function toggleRowsForList(togglesList) {
        var rows = [];
        var row = [];
        var totalSize = 0; // Total cols taken in current row
        for (var i = 0; i < togglesList.length; i++) {
            if (!togglesList[i]) continue;
            if (totalSize + togglesList[i].size > columns) {
                rows.push(row);
                row = [];
                totalSize = 0;
            }
            row.push(togglesList[i]);
            totalSize += togglesList[i].size;
        }
        if (row.length > 0) {
            rows.push(row);
        }
        return rows;
    }

    Column {
        id: contentItem
        anchors {
            fill: parent
            margins: root.padding
        }
        spacing: 12
        
        Column {
            id: usedRows
            spacing: root.spacing

            Repeater {
                id: usedRowsRepeater
                model: ScriptModel {
                    values: Array(root.toggleRows.length)
                }
                delegate: ButtonGroup {
                    id: toggleRow
                    required property int index
                    property var modelData: root.toggleRows[index]
                    property int startingIndex: {
                        const rows = root.toggleRows;
                        let sum = 0;
                        for (let i = 0; i < index; i++) {
                            sum += rows[i].length;
                        }
                        return sum;
                    }
                    spacing: root.spacing

                    Repeater {
                        model: ScriptModel {
                            values: toggleRow?.modelData ?? []
                            objectProp: "type"
                        }
                        delegate: AndroidToggleDelegateChooser {
                            startingIndex: toggleRow.startingIndex
                            editMode: root.editMode
                            quickPanel: root
                            baseCellWidth: root.baseCellWidth
                            baseCellHeight: root.baseCellHeight
                            spacing: root.spacing
                            onOpenAudioOutputDialog: root.openAudioOutputDialog()
                            onOpenAudioInputDialog: root.openAudioInputDialog()
                            onOpenBluetoothDialog: root.openBluetoothDialog()
                            onOpenNightLightDialog: root.openNightLightDialog()
                            onOpenWifiDialog: root.openWifiDialog()
                        }
                    }
                }
            }
        }

        FadeLoader {
            id: divider
            shown: root.unusedShown
            anchors {
                left: parent.left
                right: parent.right
                leftMargin: root.baseCellHeight / 2
                rightMargin: root.baseCellHeight / 2
            }
            // Slides with the toggles under it when the rows above change.
            property real shownY: y
            Behavior on shownY {
                enabled: root.movedFrom !== null
                animation: Appearance.animation.elementMove.numberAnimation.createObject(this)
            }
            transform: Translate {
                y: divider.shownY - divider.y
            }
            sourceComponent: Rectangle {
                implicitHeight: 1
                color: Appearance.colors.colOutlineVariant
            }
        }

        FadeLoader {
            id: unused
            shown: root.unusedShown
            active: root.editMode || opacity > 0
            visible: active
            onLoaded: unusedMade.restart()
            sourceComponent: Column {
                id: unusedRows
                spacing: root.spacing

                Repeater {
                    model: ScriptModel {
                        values: Array(root.unusedToggleRows.length)
                    }
                    delegate: ButtonGroup {
                        id: unusedToggleRow
                        required property int index
                        property var modelData: root.unusedToggleRows[index]
                        spacing: root.spacing

                        Repeater {
                            model: ScriptModel {
                                values: unusedToggleRow?.modelData ?? []
                                objectProp: "type"
                            }
                            delegate: AndroidToggleDelegateChooser {
                                startingIndex: -1
                                editMode: root.editMode
                                quickPanel: root
                                baseCellWidth: root.baseCellWidth
                                baseCellHeight: root.baseCellHeight
                                spacing: root.spacing
                            }
                        }
                    }
                }
            }
        }
    }
}
