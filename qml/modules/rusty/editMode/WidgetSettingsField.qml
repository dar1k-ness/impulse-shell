import QtQuick
import QtQuick.Layouts
import ri.services
import ri.modules.common
import ri.modules.common.widgets

/**
 * One setting of `WidgetSettings.fields`, with the control its schema kind calls for and its
 * validation message underneath. Emits `changed` with the entered value (sliders and text fields
 * once the edit is finished, so one edit is one undo step) and `reset` to drop the override.
 */
ColumnLayout {
    id: root
    required property var field
    signal changed(var value)
    signal reset

    readonly property bool hasError: root.field.error.length > 0
    readonly property bool bounded: root.field.min !== null && root.field.max !== null
    readonly property string shownText: root.field.input.length > 0 ? root.field.input : String(root.field.value)
    spacing: 4

    RowLayout {
        Layout.fillWidth: true
        spacing: 4

        Loader {
            Layout.fillWidth: true
            sourceComponent: root.field.kind === "bool" ? boolRow
                : root.field.kind === "int" ? intRow
                : root.field.kind === "real" ? (root.bounded ? sliderRow : textRow)
                : root.field.kind === "choice" ? (root.field.options.length <= 4 ? segmentedRow : comboRow)
                : textRow
        }

        RippleButton {
            id: resetButton
            visible: !root.field.isDefault
            implicitWidth: 36
            implicitHeight: 36
            buttonRadius: Appearance.rounding.full
            onClicked: root.reset()
            contentItem: MaterialSymbol {
                anchors.centerIn: parent
                horizontalAlignment: Text.AlignHCenter
                text: "restart_alt"
                iconSize: Appearance.font.pixelSize.larger
                color: Appearance.colors.colOnLayer2
            }
            StyledToolTip {
                text: Translation.tr("Reset to default")
            }
        }
    }

    StyledText {
        Layout.fillWidth: true
        Layout.leftMargin: 16
        visible: root.hasError
        text: root.field.error
        color: Appearance.colors.colError
        font.pixelSize: Appearance.font.pixelSize.smaller
        wrapMode: Text.Wrap
    }

    Component {
        id: boolRow
        SettingsSwitchRow {
            title: root.field.label
            checked: root.field.value === true
            onUserToggled: value => root.changed(value)
        }
    }

    Component {
        id: intRow
        SettingsSpinRow {
            title: root.field.label
            value: root.field.value
            from: root.field.min ?? -2147483648
            to: root.field.max ?? 2147483647
            stepSize: 1
            onUserChanged: value => root.changed(Math.round(value))
        }
    }

    Component {
        id: sliderRow
        SettingsRow {
            title: root.field.label
            StyledSlider {
                id: slider
                Layout.preferredWidth: 180
                configuration: StyledSlider.Configuration.XS
                from: root.field.min
                to: root.field.max
                value: root.field.value
                stopIndicatorValues: []
                usePercentTooltip: false
                tooltipContent: slider.value.toFixed(2)
                onPressedChanged: {
                    if (!pressed && value !== root.field.value)
                        root.changed(value);
                }
            }
            StyledText {
                Layout.preferredWidth: 44
                horizontalAlignment: Text.AlignRight
                text: slider.value.toFixed(2)
                color: Appearance.colors.colOnLayer2
                font.family: Appearance.font.family.numbers
                font.pixelSize: Appearance.font.pixelSize.small
            }
        }
    }

    Component {
        id: segmentedRow
        SettingsSegmentedRow {
            title: root.field.label
            options: root.field.options.map(o => ({ "label": o, "value": o }))
            currentValue: root.field.value
            onSelected: value => root.changed(value)
        }
    }

    Component {
        id: comboRow
        SettingsRow {
            title: root.field.label
            StyledComboBox {
                Layout.preferredWidth: 180
                model: root.field.options
                currentIndex: root.field.options.indexOf(root.field.value)
                onActivated: index => root.changed(root.field.options[index])
            }
        }
    }

    Component {
        id: textRow
        SettingsRow {
            title: root.field.label
            Rectangle {
                visible: root.field.kind === "color"
                implicitWidth: 24
                implicitHeight: 24
                radius: Appearance.rounding.full
                color: root.field.value
                border.width: 1
                border.color: Appearance.colors.colOutline
            }
            Rectangle {
                Layout.preferredWidth: 180
                implicitHeight: 42
                radius: Appearance.rounding.small
                color: "transparent"
                border.width: input.activeFocus || root.hasError ? 2 : 1
                border.color: root.hasError ? Appearance.colors.colError
                    : input.activeFocus ? Appearance.colors.colPrimary : Appearance.colors.colOutline
                clip: true

                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.IBeamCursor
                    onPressed: input.forceActiveFocus()
                }

                StyledTextInput {
                    id: input
                    anchors {
                        left: parent.left
                        right: parent.right
                        verticalCenter: parent.verticalCenter
                        leftMargin: 14
                        rightMargin: 14
                    }
                    clip: true
                    text: root.shownText
                    color: Appearance.colors.colOnLayer2
                    Keys.onEscapePressed: input.focus = false
                    onAccepted: input.focus = false
                    onEditingFinished: {
                        if (text !== root.shownText || root.hasError)
                            root.changed(text);
                    }

                    // Typing replaces the bound text, so a value that changes afterwards (undo,
                    // redo, reset) is put back explicitly.
                    Connections {
                        target: root
                        function onShownTextChanged() {
                            input.text = root.shownText;
                        }
                    }
                }
            }
        }
    }
}
