import QtQuick
import Rustshell
import Impulse
import QtQuick.Layouts
import ri.services
import ri.modules.common
import ri.modules.common.functions
import ri.modules.common.widgets

SettingsPage {
    id: page

    readonly property var languageNames: ({
            "de_DE": "Deutsch",
            "en_US": "English",
            "es_MX": "Español",
            "fr_FR": "Français",
            "he_HE": "עברית",
            "id_ID": "Bahasa Indonesia",
            "it_IT": "Italiano",
            "ja_JP": "日本語",
            "pt_BR": "Português (Brasil)",
            "ru_RU": "Русский",
            "tr_TR": "Türkçe",
            "uk_UA": "Українська",
            "vi_VN": "Tiếng Việt",
            "zh_CN": "简体中文"
        })

    SettingsGroup {
        title: Translation.tr("Sound")

        SettingsSwitchRow {
            id: earbangRow
            title: Translation.tr("Earbang protection")
            subtitle: Translation.tr("Stops sudden jumps in volume")
            checked: Config.options.audio.protection.enable
            onUserToggled: value => Config.options.audio.protection.enable = value
        }
        SettingsSliderRow {
            parentRow: earbangRow
            shown: Config.options.audio.protection.enable
            title: Translation.tr("Max step")
            value: Config.options.audio.protection.maxAllowedIncrease
            from: 0
            to: 100
            stepSize: 2
            onUserChanged: value => Config.options.audio.protection.maxAllowedIncrease = value
        }
        SettingsSliderRow {
            parentRow: earbangRow
            shown: Config.options.audio.protection.enable
            title: Translation.tr("Volume limit")
            value: Config.options.audio.protection.maxAllowed
            from: 0
            to: 154 // pavucontrol allows up to 153%
            stepSize: 2
            onUserChanged: value => Config.options.audio.protection.maxAllowed = value
        }
        SettingsPair {
            SettingsSwitchRow {
                shown: Battery.available
                title: Translation.tr("Battery sound")
                checked: Config.options.sounds.battery
                onUserToggled: value => Config.options.sounds.battery = value
            }
            SettingsSwitchRow {
                title: Translation.tr("Pomodoro sound")
                checked: Config.options.sounds.pomodoro
                onUserToggled: value => Config.options.sounds.pomodoro = value
            }
        }
    }

    SettingsGroup {
        title: Translation.tr("Battery")
        shown: Battery.available

        SettingsSpinRow {
            title: Translation.tr("Low warning at")
            value: Config.options.battery.low
            from: 0
            to: 100
            stepSize: 5
            unit: "%"
            onUserChanged: value => Config.options.battery.low = value
        }
        SettingsSpinRow {
            title: Translation.tr("Critical warning at")
            value: Config.options.battery.critical
            from: 0
            to: 100
            stepSize: 5
            unit: "%"
            onUserChanged: value => Config.options.battery.critical = value
        }
        SettingsSwitchRow {
            id: autoSuspendRow
            title: Translation.tr("Automatic suspend")
            subtitle: Translation.tr("Puts the PC to sleep when the battery runs low")
            checked: Config.options.battery.automaticSuspend
            onUserToggled: value => Config.options.battery.automaticSuspend = value
        }
        SettingsSpinRow {
            parentRow: autoSuspendRow
            shown: Config.options.battery.automaticSuspend
            title: Translation.tr("Suspend at")
            value: Config.options.battery.suspend
            from: 0
            to: 100
            stepSize: 5
            unit: "%"
            onUserChanged: value => Config.options.battery.suspend = value
        }
        SettingsSpinRow {
            title: Translation.tr("Full warning at")
            subtitle: Translation.tr("101% turns it off")
            value: Config.options.battery.full
            from: 0
            to: 101
            stepSize: 5
            unit: "%"
            onUserChanged: value => Config.options.battery.full = value
        }
    }

    SettingsGroup {
        title: Translation.tr("Time & language")

        SettingsSegmentedRow {
            title: Translation.tr("Clock format")
            currentValue: Config.options.time.format
            onSelected: value => {
                const hyprlockConfig = `${FileUtils.trimFileProtocol(Directories.config)}/hypr/hyprlock.conf`;
                if (value === "hh:mm") {
                    Files.renameVariable(hyprlockConfig, "TIME12", "TIME");
                } else {
                    Files.renameVariable(hyprlockConfig, "TIME", "TIME12");
                }
                Config.options.time.format = value;
            }
            options: [
                { label: Translation.tr("24h"), value: "hh:mm" },
                { label: Translation.tr("12h am/pm"), value: "h:mm ap" },
                { label: Translation.tr("12h AM/PM"), value: "h:mm AP" }
            ]
        }
        SettingsSwitchRow {
            title: Translation.tr("Show seconds")
            subtitle: Translation.tr("Uses a bit more CPU")
            checked: Config.options.time.secondPrecision
            onUserToggled: value => Config.options.time.secondPrecision = value
        }
        SettingsRow {
            title: Translation.tr("Interface language")
            subtitle: Translation.tr("\"Auto\" uses your system's locale")
            StyledComboBox {
                id: languageSelector
                buttonIcon: "language"
                Layout.fillWidth: false
                Layout.preferredWidth: 240
                textRole: "displayName"
                model: [
                    {
                        displayName: Translation.tr("Auto (System)"),
                        value: "auto"
                    },
                    ...Translation.allAvailableLanguages.map(lang => {
                        return {
                            displayName: page.languageNames[lang] ?? lang,
                            value: lang
                        };
                    })]
                currentIndex: {
                    const index = model.findIndex(item => item.value === Config.options.language.ui);
                    return index !== -1 ? index : 0;
                }
                onActivated: index => {
                    Config.options.language.ui = model[index].value;
                }
            }
        }
    }

    SettingsGroup {
        title: Translation.tr("Content")

        SettingsSwitchRow {
            title: Translation.tr("Anime content")
            subtitle: Translation.tr("Konachan and osu! wallpaper buttons in Quick, the Pictures/homework folder in the wallpaper picker")
            keywords: "weeb anime konachan osu"
            checked: Config.options.policies.weeb === 1
            onUserToggled: value => Config.options.policies.weeb = value ? 1 : 0
        }
        SettingsSwitchRow {
            title: Translation.tr("Hide clipboard images copied from sussy sources")
            checked: Config.options.workSafety.enable.clipboard
            onUserToggled: value => Config.options.workSafety.enable.clipboard = value
        }
        SettingsSwitchRow {
            title: Translation.tr("Hide sussy/anime wallpapers")
            checked: Config.options.workSafety.enable.wallpaper
            onUserToggled: value => Config.options.workSafety.enable.wallpaper = value
        }
    }
}
