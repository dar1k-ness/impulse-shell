import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Rustshell
import ri.modules.common
import ri.modules.common.functions
import ri.modules.common.widgets
import ri.modules.waffle.looks

PopupToolTip {
    id: root

    required property Item realContentItem
    realContentItem: WText {
        text: root.text
        anchors.centerIn: parent
    }

    property real visualMargin: 11
    verticalPadding: 8
    horizontalPadding: 10
    verticalMargin: visualMargin
    horizontalMargin: visualMargin

    contentItem: WToolTipContent {
        id: tooltipContent
        realContentItem: root.realContentItem
        horizontalPadding: root.horizontalPadding
        verticalPadding: root.verticalPadding
    }
}
