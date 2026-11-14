import QtQuick
import ri.services
import ri.modules.common
import ri.modules.common.widgets

SettingsPage {
    id: page

    SettingsGroup {
        title: Translation.tr("Generate colors for")

        SettingsSwitchRow {
            id: shellThemingRow
            title: Translation.tr("Shell & utilities")
            subtitle: Translation.tr("Turn this on first, the two below depend on it")
            checked: Config.options.appearance.wallpaperTheming.enableAppsAndShell
            onUserToggled: value => Config.options.appearance.wallpaperTheming.enableAppsAndShell = value
        }
        SettingsSwitchRow {
            parentRow: shellThemingRow
            shown: Config.options.appearance.wallpaperTheming.enableAppsAndShell
            title: Translation.tr("Qt apps")
            checked: Config.options.appearance.wallpaperTheming.enableQtApps
            onUserToggled: value => Config.options.appearance.wallpaperTheming.enableQtApps = value
        }
        SettingsSwitchRow {
            id: terminalThemingRow
            parentRow: shellThemingRow
            shown: Config.options.appearance.wallpaperTheming.enableAppsAndShell
            title: Translation.tr("Terminal")
            checked: Config.options.appearance.wallpaperTheming.enableTerminal
            onUserToggled: value => Config.options.appearance.wallpaperTheming.enableTerminal = value
        }
        SettingsSwitchRow {
            parentRow: terminalThemingRow
            shown: Config.options.appearance.wallpaperTheming.enableTerminal
            title: Translation.tr("Force dark mode")
            checked: Config.options.appearance.wallpaperTheming.terminalGenerationProps.forceDarkMode
            onUserToggled: value => Config.options.appearance.wallpaperTheming.terminalGenerationProps.forceDarkMode = value
        }
        SettingsSliderRow {
            parentRow: terminalThemingRow
            shown: Config.options.appearance.wallpaperTheming.enableTerminal
            title: Translation.tr("Harmony")
            subtitle: Translation.tr("How strongly colors lean to the accent")
            value: Config.options.appearance.wallpaperTheming.terminalGenerationProps.harmony * 100
            from: 0
            to: 100
            stepSize: 5
            onUserChanged: value => Config.options.appearance.wallpaperTheming.terminalGenerationProps.harmony = value / 100
        }
        SettingsSpinRow {
            parentRow: terminalThemingRow
            shown: Config.options.appearance.wallpaperTheming.enableTerminal
            title: Translation.tr("Harmonize threshold")
            value: Config.options.appearance.wallpaperTheming.terminalGenerationProps.harmonizeThreshold
            from: 0
            to: 100
            stepSize: 10
            onUserChanged: value => Config.options.appearance.wallpaperTheming.terminalGenerationProps.harmonizeThreshold = value
        }
        SettingsSliderRow {
            parentRow: terminalThemingRow
            shown: Config.options.appearance.wallpaperTheming.enableTerminal
            title: Translation.tr("Foreground boost")
            subtitle: Translation.tr("Makes text brighter")
            value: Config.options.appearance.wallpaperTheming.terminalGenerationProps.termFgBoost * 100
            from: 0
            to: 100
            stepSize: 5
            onUserChanged: value => Config.options.appearance.wallpaperTheming.terminalGenerationProps.termFgBoost = value / 100
        }
    }
}
