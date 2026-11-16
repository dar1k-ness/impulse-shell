import "periodic_table.js" as PTable
import QtQuick
import Rustshell

Item {
    id: root
    readonly property var elements: PTable.elements
    readonly property var series: PTable.series
    property real spacing: 6
    // A table that has not been shown makes its tiles one at a time, in reading order, so that
    // nothing the shell draws meanwhile waits for all of them; fill() makes the rest at once,
    // and the table is whole from then on.
    property bool whole: false
    property int made: 0
    readonly property var rowStarts: {
        let tiles = 0;
        return elements.concat(series).map(row => {
            const start = tiles;
            tiles += row.length;
            return start;
        });
    }
    readonly property int tiles: elements.concat(series).reduce((tiles, row) => tiles + row.length, 0)
    function fill() {
        whole = true;
    }
    function madeOf(row, start) {
        return whole ? row : row.slice(0, Math.max(0, made - start));
    }
    Timer {
        interval: 16
        repeat: true
        running: !root.whole && root.made < root.tiles
        onTriggered: root.made += 1
    }
    implicitWidth: mainLayout.implicitWidth
    implicitHeight: mainLayout.implicitHeight

    Column {
        id: mainLayout
        anchors.centerIn: parent
        spacing: root.spacing

        Repeater { // Main table rows
            model: root.elements
            
            delegate: Row { // Table cells
                id: tableRow
                spacing: root.spacing
                required property var modelData
                required property int index
                
                Repeater {
                    model: ScriptModel {
                        values: root.madeOf(tableRow.modelData, root.rowStarts[tableRow.index])
                    }
                    delegate: ElementTile {
                        required property var modelData
                        element: modelData
                    }

                }
            }
            
        }

        Item {
            id: gap
            implicitHeight: 20
        }

        Repeater { // Main table rows
            model: root.series
            
            delegate: Row { // Table cells
                id: seriesTableRow
                spacing: root.spacing
                required property var modelData
                required property int index
                
                Repeater {
                    model: ScriptModel {
                        values: root.madeOf(seriesTableRow.modelData, root.rowStarts[root.elements.length + seriesTableRow.index])
                    }
                    delegate: ElementTile {
                        required property var modelData
                        element: modelData
                    }

                }
            }
            
        }
    }
    
}