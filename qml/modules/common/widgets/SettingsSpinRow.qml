import QtQuick
import QtQuick.Layouts
import ri.modules.common
import ri.modules.common.widgets

/**
 * A row with a − value + stepper. Bind `value`, write it back in `onUserChanged`.
 */
SettingsRow {
    id: root
    property real value: 0
    property real from: 0
    property real to: 100
    property real stepSize: 1
    property string unit: ""
    signal userChanged(real value)

    controlSpacing: 4

    function step(dir) {
        const v = Math.max(root.from, Math.min(root.to, Math.round((root.value + dir * root.stepSize) * 1000) / 1000));
        if (v !== root.value)
            root.userChanged(v);
    }

    component StepButton: RippleButton {
        id: stepButton
        property string iconName
        implicitWidth: 36
        implicitHeight: 36
        buttonRadius: Appearance.rounding.full
        colBackground: Appearance.colors.colLayer3
        colBackgroundHover: Appearance.colors.colLayer3Hover
        colRipple: Appearance.colors.colLayer3Active
        contentItem: MaterialSymbol {
            anchors.centerIn: parent
            horizontalAlignment: Text.AlignHCenter
            text: stepButton.iconName
            iconSize: 20
            color: Appearance.colors.colOnLayer3
        }
    }

    StepButton {
        iconName: "remove"
        enabled: root.value > root.from
        onClicked: root.step(-1)
    }
    Item {
        implicitWidth: Math.max(52, valueRow.implicitWidth + 8)
        implicitHeight: 36
        Row {
            id: valueRow
            anchors.centerIn: parent
            spacing: 2
            StyledText {
                anchors.baseline: unitText.baseline
                text: Math.round(root.value * 100) / 100
                color: Appearance.colors.colOnLayer2
                font.family: Appearance.font.family.numbers
                font.pixelSize: Appearance.font.pixelSize.normal
                font.weight: Font.Medium
            }
            StyledText {
                id: unitText
                visible: root.unit.length > 0
                anchors.verticalCenter: parent.verticalCenter
                text: root.unit
                color: Appearance.colors.colSubtext
                font.pixelSize: Appearance.font.pixelSize.smaller
            }
        }
        MouseArea {
            anchors.fill: parent
            acceptedButtons: Qt.NoButton
            onWheel: wheel => root.step(wheel.angleDelta.y > 0 ? 1 : -1)
        }
    }
    StepButton {
        iconName: "add"
        enabled: root.value < root.to
        onClicked: root.step(1)
    }
}
