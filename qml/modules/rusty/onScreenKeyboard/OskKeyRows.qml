import QtQuick
import QtQuick.Layouts

ColumnLayout {
    id: keyRows
    property var rows
    spacing: 5

    Repeater {
        model: keyRows.rows

        delegate: RowLayout {
            id: keyRow
            required property var modelData
            spacing: 5
            
            Repeater {
                model: modelData
                // A normal key looks like this: {label: "a", labelShift: "A", shape: "normal", keycode: 30, type: "normal"}
                delegate: OskKey { 
                    required property var modelData
                    keyData: modelData
                }
            }
        }
    }
}
