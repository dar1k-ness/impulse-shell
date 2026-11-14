import QtQuick
import Rustshell.Io
import Impulse
import ri.services
import ri.modules.common
import ri.modules.common.widgets

SettingsPage {
    id: page
    property bool geoclueAvailable: false

    // GPS location needs GeoClue; most desktops don't have it
    GeoClueCheck {
        running: true
        onExited: code => page.geoclueAvailable = code === 0
    }

    SettingsGroup {
        title: Translation.tr("Weather")

        SettingsTextRow {
            title: Translation.tr("City")
            subtitle: Translation.tr("Empty: detect by IP")
            text: Config.options.bar.weather.city
            onEdited: value => Config.options.bar.weather.city = value
        }
        SettingsSegmentedRow {
            title: Translation.tr("Units")
            currentValue: Config.options.bar.weather.useUSCS
            onSelected: value => Config.options.bar.weather.useUSCS = value
            options: [
                { label: "°C", value: false },
                { label: "°F", value: true }
            ]
        }
        SettingsSpinRow {
            title: Translation.tr("Update every")
            value: Config.options.bar.weather.fetchInterval
            from: 5
            to: 50
            stepSize: 5
            unit: Translation.tr("min")
            onUserChanged: value => Config.options.bar.weather.fetchInterval = value
        }
        SettingsSwitchRow {
            shown: page.geoclueAvailable
            title: Translation.tr("GPS based location")
            checked: Config.options.bar.weather.enableGPS
            onUserToggled: value => Config.options.bar.weather.enableGPS = value
        }
    }

    SettingsGroup {
        title: Translation.tr("Search")

        SettingsTextRow {
            title: Translation.tr("Web search")
            subtitle: Translation.tr("Base URL, the query goes at the end")
            text: Config.options.search.engineBaseUrl
            onEdited: value => Config.options.search.engineBaseUrl = value
        }
        SettingsSwitchRow {
            title: Translation.tr("Typo-tolerant matching")
            subtitle: Translation.tr("Levenshtein instead of fuzzy; weaker with acronyms")
            checked: Config.options.search.sloppy
            onUserToggled: value => Config.options.search.sloppy = value
        }
        SettingsExpandRow {
            id: prefixesRow
            title: Translation.tr("Prefixes")
            subtitle: [Config.options.search.prefix.action, Config.options.search.prefix.clipboard, Config.options.search.prefix.emojis, Config.options.search.prefix.math, Config.options.search.prefix.shellCommand, Config.options.search.prefix.webSearch].join("  ")
        }
        SettingsTextRow {
            parentRow: prefixesRow
            shown: prefixesRow.expanded
            title: Translation.tr("Action")
            fieldWidth: 120
            text: Config.options.search.prefix.action
            onEdited: value => Config.options.search.prefix.action = value
        }
        SettingsTextRow {
            parentRow: prefixesRow
            shown: prefixesRow.expanded
            title: Translation.tr("Clipboard")
            fieldWidth: 120
            text: Config.options.search.prefix.clipboard
            onEdited: value => Config.options.search.prefix.clipboard = value
        }
        SettingsTextRow {
            parentRow: prefixesRow
            shown: prefixesRow.expanded
            title: Translation.tr("Emojis")
            fieldWidth: 120
            text: Config.options.search.prefix.emojis
            onEdited: value => Config.options.search.prefix.emojis = value
        }
        SettingsTextRow {
            parentRow: prefixesRow
            shown: prefixesRow.expanded
            title: Translation.tr("Math")
            fieldWidth: 120
            text: Config.options.search.prefix.math
            onEdited: value => Config.options.search.prefix.math = value
        }
        SettingsTextRow {
            parentRow: prefixesRow
            shown: prefixesRow.expanded
            title: Translation.tr("Shell command")
            fieldWidth: 120
            text: Config.options.search.prefix.shellCommand
            onEdited: value => Config.options.search.prefix.shellCommand = value
        }
        SettingsTextRow {
            parentRow: prefixesRow
            shown: prefixesRow.expanded
            title: Translation.tr("Web search")
            fieldWidth: 120
            text: Config.options.search.prefix.webSearch
            onEdited: value => Config.options.search.prefix.webSearch = value
        }
    }

    SettingsGroup {
        title: Translation.tr("Save to")

        SettingsTextRow {
            title: Translation.tr("Screenshots")
            subtitle: Translation.tr("Empty: only copy")
            text: Config.options.screenSnip.savePath
            onEdited: value => Config.options.screenSnip.savePath = value
        }
        SettingsTextRow {
            title: Translation.tr("Recordings")
            text: Config.options.screenRecord.savePath
            onEdited: value => Config.options.screenRecord.savePath = value
        }
    }

    SettingsGroup {
        title: Translation.tr("Other")

        SettingsSpinRow {
            title: Translation.tr("Music recognition timeout")
            value: Config.options.musicRecognition.timeout
            from: 10
            to: 100
            stepSize: 2
            unit: Translation.tr("s")
            onUserChanged: value => Config.options.musicRecognition.timeout = value
        }
        SettingsSpinRow {
            title: Translation.tr("Music recognition polling")
            value: Config.options.musicRecognition.interval
            from: 2
            to: 10
            unit: Translation.tr("s")
            onUserChanged: value => Config.options.musicRecognition.interval = value
        }
        SettingsSpinRow {
            title: Translation.tr("Resource usage polling")
            value: Config.options.resources.updateInterval
            from: 100
            to: 10000
            stepSize: 100
            unit: Translation.tr("ms")
            onUserChanged: value => Config.options.resources.updateInterval = value
        }
        SettingsTextRow {
            title: Translation.tr("User agent")
            subtitle: Translation.tr("For services that require it")
            text: Config.options.networking.userAgent
            onEdited: value => Config.options.networking.userAgent = value
        }
    }
}
