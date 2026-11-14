import QtQuick
import QtQuick.Layouts
import ri.modules.common
import ri.modules.common.widgets

/**
 * A row with a slider and its value. `valueText` formats the number shown on the right.
 */
SettingsRow {
    id: root
    property real value: 0
    property real from: 0
    property real to: 100
    property real stepSize: 1
    property string unit: "%"
    property var valueText: v => `${Math.round(v)}${root.unit}`
    signal userChanged(real value)

    StyledSlider {
        id: slider
        Layout.preferredWidth: 210
        configuration: StyledSlider.Configuration.XS
        from: root.from
        to: root.to
        stepSize: root.stepSize
        value: root.value
        stopIndicatorValues: []
        usePercentTooltip: false
        tooltipContent: root.valueText(value)
        onValueChanged: {
            if (pressed && value !== root.value)
                root.userChanged(value);
        }
        onPressedChanged: {
            if (!pressed && value !== root.value)
                root.userChanged(value);
        }
    }
    StyledText {
        Layout.preferredWidth: 44
        horizontalAlignment: Text.AlignRight
        text: root.valueText(slider.value)
        color: Appearance.colors.colOnLayer2
        font.family: Appearance.font.family.numbers
        font.pixelSize: Appearance.font.pixelSize.small
    }
}
