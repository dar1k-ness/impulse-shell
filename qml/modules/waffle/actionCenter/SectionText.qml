import QtQuick
import QtQuick.Layouts
import Rustshell
import ri
import ri.services
import ri.modules.common
import ri.modules.common.functions
import ri.modules.waffle.looks

WText {
    Layout.leftMargin: 12
    Layout.rightMargin: 12
    Layout.topMargin: 6
    Layout.bottomMargin: 6
    
    font {
        weight: Looks.font.weight.stronger
        pixelSize: Looks.font.pixelSize.large
    }
}
