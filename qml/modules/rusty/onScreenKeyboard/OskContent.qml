import ri.modules.common
import "layouts.js" as Layouts
import QtQuick
import QtQuick.Layouts

Item {
    id: root    
    property var layouts: Layouts.byName
    property var activeLayoutName: (layouts.hasOwnProperty(Config.options?.osk.layout)) 
        ? Config.options?.osk.layout 
        : Layouts.defaultLayout
    property var currentLayout: layouts[activeLayoutName]

    implicitWidth: contentRow.implicitWidth
    implicitHeight: contentRow.implicitHeight

    RowLayout {
        id: contentRow
        anchors.fill: parent
        spacing: 15

        OskKeyRows {
            Layout.fillWidth: true
            rows: root.currentLayout.keys
        }

        OskKeyRows {
            Layout.alignment: Qt.AlignBottom | Qt.AlignLeft
            rows: Layouts.numpad
        }
    }
}
