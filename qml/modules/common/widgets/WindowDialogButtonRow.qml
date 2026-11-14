import QtQuick
import QtQuick.Layouts
import Rustshell
import ri.modules.common
import ri.modules.common.functions
import ri.modules.common.widgets

RowLayout {
    id: root
    spacing: 4

    // These shouldn't be needed but it would be a terrible waste of space to follow the spec
    Layout.margins: -8
    Layout.topMargin: 0
}
