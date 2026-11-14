import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import ri.modules.common
import ri.modules.common.widgets
import ri.modules.common.functions
import ri.modules.waffle.looks

SequentialAnimation {
    id: root

    required property var target

    PropertyAction {
        target: root.target
        property: "ListView.delayRemove"
        value: true
    }
    NumberAnimation {
        target: root.target
        property: "x"
        to: root.target.width
        duration: 250
        easing.type: Easing.BezierSpline
        easing.bezierCurve: Looks.transition.easing.bezierCurve.easeIn
    }
    PropertyAction {
        target: root.target
        property: "ListView.delayRemove"
        value: false
    }
}
