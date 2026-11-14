import QtQuick
import ri.modules.common
import ri.modules.common.widgets

SettingsRow {
    id: root
    property alias options: segmented.options
    property alias currentValue: segmented.currentValue
    signal selected(var value)

    SettingsSegmented {
        id: segmented
        onSelected: value => root.selected(value)
    }
}
