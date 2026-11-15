pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Rustshell
import Impulse
import ri.modules.common
import ri.modules.common.widgets

/**
 * The right sidebar's widgets as the edit-mode layout's sidebar container places them
 * (impulse-shell only: SidebarRightContent loads this file when the layout has a sidebar container).
 * Runs of widgets narrower than the grid share one quick-panel card, packed into rows of
 * `columns` cells like the original's quick toggles; a full-width widget is a block of its own.
 * A block without an implicit height (notifications) takes the height left over, keeping at
 * least `fillMinimumHeight`; with no such block the sections keep their natural height, packed
 * to the top. What does not fit the sidebar's height is hidden, from the bottom up, so nothing
 * is laid out past the screen's edge; edit mode refuses a drop the sidebar has no room for
 * (SidebarContainer.sidebarHeights). A widget that says it is not shown (its `shown`: the
 * sliders the original's settings turn off) takes no place.
 */
Item {
    id: root
    // The monitor whose layout sidebar this lays out: the one its window shows on.
    property string connector: root.QsWindow.window?.screen?.name ?? ""
    // The sidebar's quick-toggle edit mode (the pencil button), for the widgets that have one.
    property bool editMode: false
    property real cellPadding: 6
    property real cellSpacing: 6
    property real cellHeight: 56
    property real sectionSpacing: 10
    // The least a filling block (notifications) shrinks to; below that it is hidden instead.
    // The original keeps its calendar under the notifications however little is left for them;
    // on a 720 logical high screen that is 87, and a higher minimum would hide the calendar there.
    property real fillMinimumHeight: 80
    // The column of sections, at most as tall as the sidebar.
    readonly property Item column: column
    readonly property real gridCellWidth: (root.width - root.cellPadding * 2 - root.cellSpacing * container.columns) / Math.max(1, container.columns)

    signal openAudioOutputDialog()
    signal openAudioInputDialog()
    signal openBluetoothDialog()
    signal openNightLightDialog()
    signal openWifiDialog()

    readonly property bool anySectionFills: {
        for (let i = 0; i < sectionRepeater.count; i++) {
            const s = sectionRepeater.itemAt(i);
            if (s && s.fills && s.visible)
                return true;
        }
        return false;
    }

    // Which rows of each section fit the sidebar's height, sections laid out top to bottom:
    // `kept[i]` rows of section i show (a block counts as one row), the first that does not fit
    // and everything after it are hidden (their ids in `hidden`). Each row is charged its
    // height, a filling block the least it keeps; SidebarContainer.sidebarHeights charges a
    // drop the same.
    readonly property var fit: {
        const kept = [];
        const hidden = [];
        let used = 0;
        let full = false;
        // Not laid out yet: nothing to judge against.
        const unbounded = root.height <= 0;
        for (let i = 0; i < sectionRepeater.count; i++) {
            const s = sectionRepeater.itemAt(i);
            const lead = used > 0 ? root.sectionSpacing : 0;
            let k = 0;
            if (s && s.grid) {
                let h = lead + root.cellPadding * 2;
                const heights = s.rowHeights;
                for (let r = 0; r < heights.length; r++) {
                    const add = (r > 0 ? root.cellSpacing : 0) + heights[r];
                    if (!full && (unbounded || used + h + add <= root.height + 1e-6)) {
                        h += add;
                        k++;
                    } else {
                        full = true;
                        for (const cell of s.modelData.rows[r])
                            hidden.push(cell.id);
                    }
                }
                if (k > 0)
                    used += h;
            } else if (s && !s.shown) {
                // A block that says it is not shown: charged nothing, and not one the height hid.
                k = 1;
            } else if (s) {
                if (!full && (unbounded || used + lead + s.cost <= root.height + 1e-6)) {
                    used += lead + s.cost;
                    k = 1;
                } else {
                    full = true;
                    hidden.push(s.modelData.rows[0][0].id);
                }
            }
            kept.push(k);
        }
        return {
            "kept": kept,
            "hidden": hidden,
            "used": used
        };
    }
    readonly property string hiddenIds: root.fit.hidden.join(", ")
    onHiddenIdsChanged: {
        if (root.hiddenIds.length > 0)
            console.info("[Sidebar] " + root.connector + " has no room for " + root.hiddenIds + ": hidden");
        else
            console.info("[Sidebar] " + root.connector + " shows all its widgets");
    }

    ColumnLayout {
        id: column
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        height: root.anySectionFills ? root.height : column.implicitHeight
        spacing: root.sectionSpacing

        SidebarContainer {
            id: container
            connector: root.connector
            available: root.height
            cellPadding: root.cellPadding
            cellSpacing: root.cellSpacing
            sectionSpacing: root.sectionSpacing
            // What each widget the sidebar shows costs it as `fit` charges it, and what a drop
            // of each of the monitor's other widgets would (laid out off screen in `probes`).
            sidebarHeights: {
                const heights = {};
                for (let i = 0; i < sectionRepeater.count; i++) {
                    const s = sectionRepeater.itemAt(i);
                    if (!s)
                        continue;
                    if (!s.grid) {
                        heights[s.modelData.rows[0][0].id] = s.cost;
                        continue;
                    }
                    const cells = s.cellHeights;
                    for (const id in cells)
                        heights[id] = cells[id];
                }
                for (let j = 0; j < probeRepeater.count; j++) {
                    const p = probeRepeater.itemAt(j);
                    if (p)
                        heights[p.cellData.id] = p.cost;
                }
                return heights;
            }
        }

        Repeater {
            id: sectionRepeater
            model: container.sections
            delegate: Rectangle {
                id: section
                required property var modelData
                required property int index
                readonly property bool grid: section.modelData.grid
                readonly property bool shown: section.grid || block.shown
                readonly property bool fills: !section.grid && block.shown && block.implicitHeight <= 0
                // The height `fit` charges a block.
                readonly property real cost: section.fills ? root.fillMinimumHeight : block.implicitHeight
                readonly property int kept: root.fit.kept[section.index] ?? 0
                // By id, the height of each cell of the section's rows.
                readonly property var cellHeights: {
                    const heights = {};
                    for (let r = 0; r < rowRepeater.count; r++) {
                        const cells = rowRepeater.itemAt(r)?.cellHeights ?? {};
                        for (const id in cells)
                            heights[id] = cells[id];
                    }
                    return heights;
                }
                readonly property var rowHeights: {
                    const heights = [];
                    for (let r = 0; r < rowRepeater.count; r++)
                        heights.push(rowRepeater.itemAt(r)?.implicitHeight ?? 0);
                    return heights;
                }

                visible: section.kept > 0 && section.shown
                Layout.fillWidth: true
                Layout.fillHeight: section.fills
                Layout.minimumHeight: section.fills ? root.fillMinimumHeight : -1
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
                        id: rowRepeater
                        model: section.grid ? section.modelData.rows : []
                        delegate: ButtonGroup {
                            id: row
                            required property var modelData
                            required property int index
                            readonly property var cellHeights: {
                                const heights = {};
                                for (let c = 0; c < cellRepeater.count; c++) {
                                    const cell = cellRepeater.itemAt(c);
                                    if (cell)
                                        heights[cell.cellData.id] = cell.implicitHeight;
                                }
                                return heights;
                            }
                            visible: row.index < section.kept
                            spacing: root.cellSpacing

                            Repeater {
                                id: cellRepeater
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
                                        "baseCellWidth": root.gridCellWidth,
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
    }

    // Edit mode: the monitor's other widgets in the form a drop would give them, never shown.
    Item {
        id: probes
        visible: false
        width: root.width

        Repeater {
            id: probeRepeater
            model: container.probes
            delegate: SidebarCell {
                id: probe
                required property var modelData
                readonly property bool grid: (probe.modelData.size ?? 1) < container.columns
                // What `fit` would charge it: a grid cell's height, a block's or the least a
                // filling block keeps.
                readonly property real cost: probe.grid || probe.implicitHeight > 0 ? probe.implicitHeight : root.fillMinimumHeight
                probe: true
                cellData: probe.modelData
                placement: probe.grid ? ({
                        "cellSize": probe.modelData.size,
                        "baseCellWidth": root.gridCellWidth,
                        "baseCellHeight": root.cellHeight,
                        "cellSpacing": root.cellSpacing
                    }) : ({
                        "editMode": root.editMode
                    })
                width: probe.grid ? implicitWidth : probes.width
                height: implicitHeight
            }
        }
    }

    // One widget: the WidgetHost for its type and settings, with its dialog requests forwarded.
    component SidebarCell: Item {
        id: cell
        required property var cellData
        property var placement: ({})
        // Laid out only to be measured: not a widget of the layout edit mode may pick.
        property bool probe: false
        // The widget's own `shown`, not its `visible`: that one reads back this cell's visibility.
        readonly property bool shown: host.item?.shown ?? true
        implicitWidth: host.implicitWidth
        implicitHeight: host.implicitHeight

        WidgetHost {
            id: host
            anchors.fill: parent
            surface: "sidebar"
            widgetType: cell.cellData.type ?? ""
            layoutId: cell.probe ? "" : (cell.cellData.id ?? "")
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
