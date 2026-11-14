import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Rustshell
import ri
import ri.services
import ri.modules.common
import ri.modules.common.functions
import ri.modules.common.widgets
import ri.modules.waffle.looks
import ri.modules.waffle.actionCenter

WChoiceButton {
    id: root

    property bool expanded: false
    checked: expanded
    clip: true

    horizontalPadding: 12
    verticalPadding: 6
    animateChoiceHighlight: false

    Behavior on implicitHeight {
        animation: Looks.transition.resize.createObject(this)
    }
    onClicked: expanded = !expanded
}
