import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Rustshell
import ri.modules.common
import ri.modules.common.functions
import ri.modules.common.widgets
import ri.modules.waffle.looks

StyledToolTip {
    id: root

    required property Item realContentItem
    font {
        family: Looks.font.family.ui
        pixelSize: Looks.font.pixelSize.normal
        weight: Looks.font.weight.regular
    }
    realContentItem: WText {
        text: root.text
        font: root.font
        anchors.centerIn: parent
    }

    verticalPadding: 8
    horizontalPadding: 10

    delay: 400

    contentItem: WToolTipContent {
        id: tooltipContent
        realContentItem: root.realContentItem
        horizontalPadding: root.horizontalPadding
        verticalPadding: root.verticalPadding
    }
}
