pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Impulse
import ri.modules.common
import ri.modules.common.widgets

/**
 * The right sidebar's widgets as the edit-mode layout's sidebar container places them
 * (rusty-impulse only: SidebarRightContent loads this file when the layout has a sidebar container).
 * Runs of widgets narrower than the grid share one quick-panel card, packed into rows of
 * `columns` cells like the original's quick toggles; a full-width widget is a block of its own.
 * A block without an implicit height (notifications) takes the height left over; with no
 * such block the sections keep their natural height, packed to the top. A widget that says it
 * is not shown (its `shown`: the sliders the original's settings turn off) takes no place.
 */
ColumnLayout {
    id: root
    property string connector: ""
    // The sidebar's quick-toggle edit mode (the pencil button), for the widgets that have one.
    property bool editMode: false
    property real cellPadding: 6
    property real cellSpacing: 6
    property real cellHeight: 56
    property real sectionSpacing: 10

    signal openAudioOutputDialog()
    signal openAudioInputDialog()
    signal openBluetoothDialog()
    signal openNightLightDialog()
    signal openWifiDialog()

    readonly property bool anySectionFills: {
        for (let i = 0; i < sectionRepeater.count; i++) {
            const s = sectionRepeater.itemAt(i);
            if (s && s.fills)
                return true;
        }
        return false;
    }

    spacing: root.sectionSpacing

    SidebarContainer {
        id: container
        connector: root.connector
    }

    Repeater {
        id: sectionRepeater
        model: container.sections
        delegate: Rectangle {
            id: section
            required property var modelData
            readonly property bool grid: section.modelData.grid
            readonly property bool fills: !section.grid && block.shown && block.implicitHeight <= 0
            readonly property real cellWidth: (section.width - root.cellPadding * 2 - root.cellSpacing * container.columns) / container.columns

            visible: section.grid || block.shown
            Layout.fillWidth: true
            Layout.fillHeight: section.fills
            implicitHeight: section.grid ? rows.implicitHeight + root.cellPadding * 2 : block.implicitHeight
            radius: Appearance.rounding.normal
            color: section.grid ? Appearance.colors.colLayer1 : "transparent"

            Column {
                id: rows
                visible: section.grid
                anchors.fill: parent
                anchors.margins: root.cellPadding
                spacing: root.cellSpacing

                Repeater {
                    model: section.grid ? section.modelData.rows : []
                    delegate: ButtonGroup {
                        id: row
                        required property var modelData
                        spacing: root.cellSpacing

                        Repeater {
                            model: row.modelData
                            delegate: SidebarCell {
                                required property var modelData
                                required property int index
                                // GroupButton's own rule, applied to the cell that wraps it: the
                                // button next to the clicked one (the first when none is) fills.
                                Layout.fillWidth: row.clickIndex - 1 <= index && index <= row.clickIndex + 1
                                Layout.fillHeight: row.clickIndex - 1 <= index && index <= row.clickIndex + 1
                                visible: shown
                                cellData: modelData
                                placement: ({
                                    "cellSize": modelData.size,
                                    "baseCellWidth": section.cellWidth,
                                    "baseCellHeight": root.cellHeight,
                                    "cellSpacing": root.cellSpacing
                                })
                                width: implicitWidth
                                height: implicitHeight
                            }
                        }
                    }
                }
            }

            SidebarCell {
                id: block
                visible: !section.grid && block.shown
                anchors.fill: parent
                cellData: section.grid ? ({}) : section.modelData.rows[0][0]
                placement: ({
                    "editMode": root.editMode
                })
            }
        }
    }

    Item {
        visible: !root.anySectionFills
        Layout.fillWidth: true
        Layout.fillHeight: true
    }

    // One widget: the WidgetHost for its type and settings, with its dialog requests forwarded.
    component SidebarCell: Item {
        id: cell
        required property var cellData
        property var placement: ({})
        // The widget's own `shown`, not its `visible`: that one reads back this cell's visibility.
        readonly property bool shown: host.item?.shown ?? true
        implicitWidth: host.implicitWidth
        implicitHeight: host.implicitHeight

        WidgetHost {
            id: host
            anchors.fill: parent
            widgetType: cell.cellData.type ?? ""
            settings: cell.cellData.settings ?? ({})
            placement: cell.placement
        }

        Connections {
            target: host.item
            ignoreUnknownSignals: true
            function onOpenAudioOutputDialog() {
                root.openAudioOutputDialog();
            }
            function onOpenAudioInputDialog() {
                root.openAudioInputDialog();
            }
            function onOpenBluetoothDialog() {
                root.openBluetoothDialog();
            }
            function onOpenNightLightDialog() {
                root.openNightLightDialog();
            }
            function onOpenWifiDialog() {
                root.openWifiDialog();
            }
        }
    }
}
