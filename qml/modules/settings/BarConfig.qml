import QtQuick
import QtQuick.Layouts
import ri.services
import ri.modules.common
import ri.modules.common.widgets

SettingsPage {
    id: page

    SettingsGroup {
        title: Translation.tr("Position & look")

        SettingsCard {
            StyledText {
                text: Translation.tr("Position")
                color: Appearance.colors.colOnLayer2
                font.pixelSize: Appearance.font.pixelSize.small
                font.weight: Font.Medium
            }
            RowLayout {
                spacing: 2
                SettingsBarPositionTile { position: 0; label: Translation.tr("Top") }
                SettingsBarPositionTile { position: 1; label: Translation.tr("Bottom") }
                SettingsBarPositionTile { position: 2; label: Translation.tr("Left") }
                SettingsBarPositionTile { position: 3; label: Translation.tr("Right") }
            }
        }
        SettingsSegmentedRow {
            title: Translation.tr("Style")
            subtitle: Translation.tr("How the bar meets the screen edge")
            currentValue: Config.options.bar.cornerStyle
            onSelected: value => Config.options.bar.cornerStyle = value
            options: [
                { label: Translation.tr("Hug"), value: 0 },
                { label: Translation.tr("Float"), value: 1 },
                { label: Translation.tr("Rect"), value: 2 }
            ]
        }
        SettingsSegmentedRow {
            title: Translation.tr("Group style")
            currentValue: Config.options.bar.borderless
            onSelected: value => Config.options.bar.borderless = value
            options: [
                { label: Translation.tr("Pills"), value: false },
                { label: Translation.tr("Flat"), value: true }
            ]
        }
        SettingsSwitchRow {
            title: Translation.tr("Hide automatically")
            subtitle: Translation.tr("Shows when the cursor touches the edge")
            checked: Config.options.bar.autoHide.enable
            onUserToggled: value => Config.options.bar.autoHide.enable = value
        }
    }

    SettingsGroup {
        title: Translation.tr("Workspaces")

        SettingsSpinRow {
            title: Translation.tr("Workspaces shown")
            value: Config.options.bar.workspaces.shown
            from: 1
            to: 30
            onUserChanged: value => Config.options.bar.workspaces.shown = value
        }
        SettingsSegmentedRow {
            title: Translation.tr("Number style")
            currentValue: JSON.stringify(Config.options.bar.workspaces.numberMap)
            onSelected: value => Config.options.bar.workspaces.numberMap = JSON.parse(value)
            options: [
                { label: "1 2 3", value: '[]' },
                { label: "一 二 三", value: '["一","二","三","四","五","六","七","八","九","十","十一","十二","十三","十四","十五","十六","十七","十八","十九","二十"]' },
                { label: "I II III", value: '["I","II","III","IV","V","VI","VII","VIII","IX","X","XI","XII","XIII","XIV","XV","XVI","XVII","XVIII","XIX","XX"]' }
            ]
        }
        SettingsSwitchRow {
            id: alwaysNumbersRow
            title: Translation.tr("Always show numbers")
            checked: Config.options.bar.workspaces.alwaysShowNumbers
            onUserToggled: value => Config.options.bar.workspaces.alwaysShowNumbers = value
        }
        SettingsSpinRow {
            parentRow: alwaysNumbersRow
            shown: !Config.options.bar.workspaces.alwaysShowNumbers
            title: Translation.tr("Numbers while holding Super")
            subtitle: Translation.tr("Delay before they appear")
            value: Config.options.bar.workspaces.showNumberDelay
            from: 0
            to: 1000
            stepSize: 50
            unit: Translation.tr("ms")
            onUserChanged: value => Config.options.bar.workspaces.showNumberDelay = value
        }
        SettingsSwitchRow {
            id: appIconsRow
            title: Translation.tr("Show app icons")
            checked: Config.options.bar.workspaces.showAppIcons
            onUserToggled: value => Config.options.bar.workspaces.showAppIcons = value
        }
        SettingsSwitchRow {
            parentRow: appIconsRow
            shown: Config.options.bar.workspaces.showAppIcons
            title: Translation.tr("Tint app icons")
            checked: Config.options.bar.workspaces.monochromeIcons
            onUserToggled: value => Config.options.bar.workspaces.monochromeIcons = value
        }
    }

    SettingsGroup {
        title: Translation.tr("Buttons on the bar")

        SettingsPair {
            SettingsSwitchRow {
                title: Translation.tr("Screen snip")
                checked: Config.options.bar.utilButtons.showScreenSnip
                onUserToggled: value => Config.options.bar.utilButtons.showScreenSnip = value
            }
            SettingsSwitchRow {
                title: Translation.tr("Color picker")
                checked: Config.options.bar.utilButtons.showColorPicker
                onUserToggled: value => Config.options.bar.utilButtons.showColorPicker = value
            }
        }
        SettingsPair {
            SettingsSwitchRow {
                title: Translation.tr("Record screen")
                checked: Config.options.bar.utilButtons.showScreenRecord
                onUserToggled: value => Config.options.bar.utilButtons.showScreenRecord = value
            }
            SettingsSwitchRow {
                title: Translation.tr("Mic toggle")
                checked: Config.options.bar.utilButtons.showMicToggle
                onUserToggled: value => Config.options.bar.utilButtons.showMicToggle = value
            }
        }
        SettingsPair {
            SettingsSwitchRow {
                title: Translation.tr("Keyboard toggle")
                checked: Config.options.bar.utilButtons.showKeyboardToggle
                onUserToggled: value => Config.options.bar.utilButtons.showKeyboardToggle = value
            }
            SettingsSwitchRow {
                title: Translation.tr("Dark/Light toggle")
                checked: Config.options.bar.utilButtons.showDarkModeToggle
                onUserToggled: value => Config.options.bar.utilButtons.showDarkModeToggle = value
            }
        }
        SettingsPair {
            SettingsSwitchRow {
                title: Translation.tr("Performance profile")
                checked: Config.options.bar.utilButtons.showPerformanceProfileToggle
                onUserToggled: value => Config.options.bar.utilButtons.showPerformanceProfileToggle = value
            }
            SettingsSwitchRow {
                title: Translation.tr("Weather")
                checked: Config.options.bar.weather.enable
                onUserToggled: value => Config.options.bar.weather.enable = value
            }
        }
    }

    SettingsGroup {
        title: Translation.tr("Tray & indicators")

        SettingsSwitchRow {
            title: Translation.tr("Make icons pinned by default")
            checked: Config.options.tray.invertPinnedItems
            onUserToggled: value => Config.options.tray.invertPinnedItems = value
        }
        SettingsSwitchRow {
            title: Translation.tr("Tint tray icons")
            checked: Config.options.tray.monochromeIcons
            onUserToggled: value => Config.options.tray.monochromeIcons = value
        }
        SettingsSwitchRow {
            title: Translation.tr("Unread notifications count")
            checked: Config.options.bar.indicators.notifications.showUnreadCount
            onUserToggled: value => Config.options.bar.indicators.notifications.showUnreadCount = value
        }
        SettingsSwitchRow {
            title: Translation.tr("Tooltips on click")
            subtitle: Translation.tr("Instead of on hover")
            checked: Config.options.bar.tooltips.clickToShow
            onUserToggled: value => Config.options.bar.tooltips.clickToShow = value
        }
    }
}
