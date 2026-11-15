import QtQuick
import QtQuick.Layouts
import ri.services
import ri.modules.common
import ri.modules.common.widgets

SettingsPage {
    id: page
    property int currentTab: 0

    SettingsTabBar {
        currentIndex: page.currentTab
        onTabClicked: index => page.currentTab = index
        tabs: [
            { name: Translation.tr("Dock"), icon: "call_to_action" },
            { name: Translation.tr("Lock screen"), icon: "lock" },
            { name: Translation.tr("Notifications"), icon: "notifications" },
            { name: Translation.tr("Sidebars"), icon: "side_navigation" },
            { name: Translation.tr("Overview"), icon: "overview_key" },
            { name: Translation.tr("Screenshot"), icon: "screenshot_region" },
            { name: Translation.tr("Cheat sheet"), icon: "keyboard" },
            { name: Translation.tr("Fonts"), icon: "match_case" }
        ]
    }

    SettingsTab {
        tabIndex: 0
        tabName: Translation.tr("Dock")
        visible: page.currentTab === tabIndex

        SettingsGroup {
            title: Translation.tr("Dock")
            SettingsSwitchRow {
                id: dockRow
                title: Translation.tr("Show dock")
                subtitle: Translation.tr("Bar with pinned and open apps at the bottom of the screen")
                checked: Config.options.dock.enable
                onUserToggled: value => Config.options.dock.enable = value
            }
            SettingsSwitchRow {
                parentRow: dockRow
                shown: Config.options.dock.enable
                title: Translation.tr("Hover to reveal")
                subtitle: Translation.tr("Hidden until the cursor reaches the bottom edge")
                checked: Config.options.dock.hoverToReveal
                onUserToggled: value => Config.options.dock.hoverToReveal = value
            }
            SettingsSwitchRow {
                parentRow: dockRow
                shown: Config.options.dock.enable
                title: Translation.tr("Pinned on startup")
                subtitle: Translation.tr("Stays visible instead of hiding under windows")
                checked: Config.options.dock.pinnedOnStartup
                onUserToggled: value => Config.options.dock.pinnedOnStartup = value
            }
            SettingsSwitchRow {
                parentRow: dockRow
                shown: Config.options.dock.enable
                title: Translation.tr("Tint app icons")
                checked: Config.options.dock.monochromeIcons
                onUserToggled: value => Config.options.dock.monochromeIcons = value
            }
        }
    }

    SettingsTab {
        tabIndex: 1
        tabName: Translation.tr("Lock screen")
        visible: page.currentTab === tabIndex

        SettingsGroup {
            title: Translation.tr("Look")
            SettingsSwitchRow {
                id: lockBlurRow
                title: Translation.tr("Blur the wallpaper")
                checked: Config.options.lock.blur.enable
                onUserToggled: value => Config.options.lock.blur.enable = value
            }
            SettingsSliderRow {
                parentRow: lockBlurRow
                shown: Config.options.lock.blur.enable
                title: Translation.tr("Extra zoom")
                value: Config.options.lock.blur.extraZoom * 100
                from: 1
                to: 150
                stepSize: 1
                onUserChanged: value => Config.options.lock.blur.extraZoom = value / 100
            }
            SettingsSwitchRow {
                title: Translation.tr("Center clock")
                checked: Config.options.lock.centerClock
                onUserToggled: value => Config.options.lock.centerClock = value
            }
            SettingsSwitchRow {
                title: Translation.tr("Show \"Locked\" text")
                checked: Config.options.lock.showLockedText
                onUserToggled: value => Config.options.lock.showLockedText = value
            }
            SettingsSwitchRow {
                title: Translation.tr("Varying shapes for password characters")
                checked: Config.options.lock.materialShapeChars
                onUserToggled: value => Config.options.lock.materialShapeChars = value
            }
        }
        SettingsGroup {
            title: Translation.tr("Security")
            SettingsSwitchRow {
                title: Translation.tr("Unlock the keyring too")
                subtitle: Translation.tr("Needed by the browser and apps that keep passwords there")
                checked: Config.options.lock.security.unlockKeyring
                onUserToggled: value => Config.options.lock.security.unlockKeyring = value
            }
            SettingsSwitchRow {
                title: Translation.tr("Ask for password before power off")
                subtitle: Translation.tr("Makes accidental shutdowns harder")
                checked: Config.options.lock.security.requirePasswordToPower
                onUserToggled: value => Config.options.lock.security.requirePasswordToPower = value
            }
            SettingsSwitchRow {
                title: Translation.tr("Lock on startup")
                checked: Config.options.lock.launchOnStartup
                onUserToggled: value => Config.options.lock.launchOnStartup = value
            }
            SettingsSwitchRow {
                title: Translation.tr("Use Hyprlock")
                subtitle: Translation.tr("Instead of the built-in lock screen, e.g. for fingerprint unlock")
                checked: Config.options.lock.useHyprlock
                onUserToggled: value => Config.options.lock.useHyprlock = value
            }
        }
    }

    SettingsTab {
        tabIndex: 2
        tabName: Translation.tr("Notifications")
        visible: page.currentTab === tabIndex

        SettingsGroup {
            title: Translation.tr("Notifications")
            SettingsSpinRow {
                title: Translation.tr("Hide after")
                subtitle: Translation.tr("When the notification doesn't set its own time")
                value: Config.options.notifications.timeout / 1000
                from: 1
                to: 60
                unit: Translation.tr("s")
                onUserChanged: value => Config.options.notifications.timeout = value * 1000
            }
            SettingsSwitchRow {
                id: forceMonitorRow
                title: Translation.tr("Only on one monitor")
                checked: Config.options.notifications.forceMonitor.enable
                onUserToggled: value => Config.options.notifications.forceMonitor.enable = value
            }
            SettingsTextRow {
                parentRow: forceMonitorRow
                shown: Config.options.notifications.forceMonitor.enable
                title: Translation.tr("Monitor name")
                fieldWidth: 180
                placeholderText: "eDP-1"
                text: Config.options.notifications.forceMonitor.name
                onEdited: value => Config.options.notifications.forceMonitor.name = value
            }
        }
        SettingsGroup {
            title: Translation.tr("On-screen display")
            SettingsSpinRow {
                title: Translation.tr("Volume and brightness popup")
                subtitle: Translation.tr("How long it stays")
                value: Config.options.osd.timeout
                from: 100
                to: 3000
                stepSize: 100
                unit: Translation.tr("ms")
                onUserChanged: value => Config.options.osd.timeout = value
            }
        }
    }

    SettingsTab {
        tabIndex: 3
        tabName: Translation.tr("Sidebars")
        visible: page.currentTab === tabIndex

        SettingsGroup {
            title: Translation.tr("Sidebars")
            SettingsSwitchRow {
                title: Translation.tr("Keep right sidebar loaded")
                subtitle: Translation.tr("Opens faster, uses about 15MB more memory")
                checked: Config.options.sidebar.keepRightSidebarLoaded
                onUserToggled: value => Config.options.sidebar.keepRightSidebarLoaded = value
            }
            SettingsSwitchRow {
                title: Translation.tr("Translator")
                checked: Config.options.sidebar.translator.enable
                onUserToggled: value => Config.options.sidebar.translator.enable = value
            }
        }
        SettingsGroup {
            title: Translation.tr("Quick toggles")
            SettingsSegmentedRow {
                id: togglesStyleRow
                title: Translation.tr("Style")
                currentValue: Config.options.sidebar.quickToggles.style
                onSelected: value => Config.options.sidebar.quickToggles.style = value
                options: [
                    { label: Translation.tr("Classic"), value: "classic" },
                    { label: Translation.tr("Android"), value: "android" }
                ]
            }
            SettingsSpinRow {
                parentRow: togglesStyleRow
                shown: Config.options.sidebar.quickToggles.style === "android"
                title: Translation.tr("Columns")
                value: Config.options.sidebar.quickToggles.android.columns
                from: 1
                to: 8
                onUserChanged: value => Config.options.sidebar.quickToggles.android.columns = value
            }
            SettingsSwitchRow {
                id: slidersRow
                title: Translation.tr("Sliders")
                checked: Config.options.sidebar.quickSliders.enable
                onUserToggled: value => Config.options.sidebar.quickSliders.enable = value
            }
            SettingsSwitchRow {
                parentRow: slidersRow
                shown: Config.options.sidebar.quickSliders.enable
                title: Translation.tr("Brightness")
                checked: Config.options.sidebar.quickSliders.showBrightness
                onUserToggled: value => Config.options.sidebar.quickSliders.showBrightness = value
            }
            SettingsSwitchRow {
                parentRow: slidersRow
                shown: Config.options.sidebar.quickSliders.enable
                title: Translation.tr("Volume")
                checked: Config.options.sidebar.quickSliders.showVolume
                onUserToggled: value => Config.options.sidebar.quickSliders.showVolume = value
            }
            SettingsSwitchRow {
                parentRow: slidersRow
                shown: Config.options.sidebar.quickSliders.enable
                title: Translation.tr("Microphone")
                checked: Config.options.sidebar.quickSliders.showMic
                onUserToggled: value => Config.options.sidebar.quickSliders.showMic = value
            }
        }
        SettingsGroup {
            title: Translation.tr("Open from screen corners")
            SettingsSwitchRow {
                id: cornerRow
                title: Translation.tr("Corner open")
                checked: Config.options.sidebar.cornerOpen.enable
                onUserToggled: value => Config.options.sidebar.cornerOpen.enable = value
            }
            SettingsSwitchRow {
                id: clicklessRow
                parentRow: cornerRow
                shown: Config.options.sidebar.cornerOpen.enable
                title: Translation.tr("Hover to trigger")
                subtitle: Translation.tr("Off: you have to click")
                checked: Config.options.sidebar.cornerOpen.clickless
                onUserToggled: value => Config.options.sidebar.cornerOpen.clickless = value
            }
            SettingsSwitchRow {
                id: cornerEndRow
                parentRow: clicklessRow
                shown: Config.options.sidebar.cornerOpen.enable && !Config.options.sidebar.cornerOpen.clickless
                title: Translation.tr("Hover the very corner to open")
                checked: Config.options.sidebar.cornerOpen.clicklessCornerEnd
                onUserToggled: value => Config.options.sidebar.cornerOpen.clicklessCornerEnd = value
            }
            SettingsSpinRow {
                parentRow: cornerEndRow
                shown: Config.options.sidebar.cornerOpen.enable && !Config.options.sidebar.cornerOpen.clickless && Config.options.sidebar.cornerOpen.clicklessCornerEnd
                title: Translation.tr("Vertical offset")
                value: Config.options.sidebar.cornerOpen.clicklessCornerVerticalOffset
                from: 0
                to: 20
                unit: Translation.tr("px")
                onUserChanged: value => Config.options.sidebar.cornerOpen.clicklessCornerVerticalOffset = value
            }
            SettingsSwitchRow {
                parentRow: cornerRow
                shown: Config.options.sidebar.cornerOpen.enable
                title: Translation.tr("Bottom corners")
                checked: Config.options.sidebar.cornerOpen.bottom
                onUserToggled: value => Config.options.sidebar.cornerOpen.bottom = value
            }
            SettingsSwitchRow {
                parentRow: cornerRow
                shown: Config.options.sidebar.cornerOpen.enable
                title: Translation.tr("Scroll for brightness and volume")
                checked: Config.options.sidebar.cornerOpen.valueScroll
                onUserToggled: value => Config.options.sidebar.cornerOpen.valueScroll = value
            }
            SettingsSwitchRow {
                parentRow: cornerRow
                shown: Config.options.sidebar.cornerOpen.enable
                title: Translation.tr("Visualize region")
                checked: Config.options.sidebar.cornerOpen.visualize
                onUserToggled: value => Config.options.sidebar.cornerOpen.visualize = value
            }
            SettingsSpinRow {
                parentRow: cornerRow
                shown: Config.options.sidebar.cornerOpen.enable
                title: Translation.tr("Region width")
                value: Config.options.sidebar.cornerOpen.cornerRegionWidth
                from: 1
                to: 300
                stepSize: 5
                unit: Translation.tr("px")
                onUserChanged: value => Config.options.sidebar.cornerOpen.cornerRegionWidth = value
            }
            SettingsSpinRow {
                parentRow: cornerRow
                shown: Config.options.sidebar.cornerOpen.enable
                title: Translation.tr("Region height")
                value: Config.options.sidebar.cornerOpen.cornerRegionHeight
                from: 1
                to: 300
                stepSize: 1
                unit: Translation.tr("px")
                onUserChanged: value => Config.options.sidebar.cornerOpen.cornerRegionHeight = value
            }
        }
    }

    SettingsTab {
        tabIndex: 4
        tabName: Translation.tr("Overview")
        visible: page.currentTab === tabIndex

        SettingsGroup {
            title: Translation.tr("Overview")
            SettingsSwitchRow {
                id: overviewRow
                title: Translation.tr("Enable overview")
                checked: Config.options.overview.enable
                onUserToggled: value => Config.options.overview.enable = value
            }
            SettingsSpinRow {
                parentRow: overviewRow
                shown: Config.options.overview.enable
                title: Translation.tr("Scale")
                value: Math.round(Config.options.overview.scale * 100)
                from: 1
                to: 100
                unit: "%"
                onUserChanged: value => Config.options.overview.scale = value / 100
            }
            SettingsPair {
                shown: Config.options.overview.enable
                SettingsSpinRow {
                    parentRow: overviewRow
                    title: Translation.tr("Rows")
                    value: Config.options.overview.rows
                    from: 1
                    to: 20
                    onUserChanged: value => Config.options.overview.rows = value
                }
                SettingsSpinRow {
                    parentRow: overviewRow
                    title: Translation.tr("Columns")
                    value: Config.options.overview.columns
                    from: 1
                    to: 20
                    onUserChanged: value => Config.options.overview.columns = value
                }
            }
            SettingsSegmentedRow {
                parentRow: overviewRow
                shown: Config.options.overview.enable
                title: Translation.tr("Horizontal order")
                currentValue: Config.options.overview.orderRightLeft
                onSelected: value => Config.options.overview.orderRightLeft = value
                options: [
                    { label: Translation.tr("Left to right"), value: false },
                    { label: Translation.tr("Right to left"), value: true }
                ]
            }
            SettingsSegmentedRow {
                parentRow: overviewRow
                shown: Config.options.overview.enable
                title: Translation.tr("Vertical order")
                currentValue: Config.options.overview.orderBottomUp
                onSelected: value => Config.options.overview.orderBottomUp = value
                options: [
                    { label: Translation.tr("Top-down"), value: false },
                    { label: Translation.tr("Bottom-up"), value: true }
                ]
            }
            SettingsSwitchRow {
                parentRow: overviewRow
                shown: Config.options.overview.enable
                title: Translation.tr("Center icons")
                checked: Config.options.overview.centerIcons
                onUserToggled: value => Config.options.overview.centerIcons = value
            }
        }
        SettingsGroup {
            title: Translation.tr("Overlay (Super+G)")
            SettingsSwitchRow {
                title: Translation.tr("Opening zoom animation")
                checked: Config.options.overlay.openingZoomAnimation
                onUserToggled: value => Config.options.overlay.openingZoomAnimation = value
            }
            SettingsSwitchRow {
                title: Translation.tr("Darken screen")
                checked: Config.options.overlay.darkenScreen
                onUserToggled: value => Config.options.overlay.darkenScreen = value
            }
            SettingsTextRow {
                title: Translation.tr("Crosshair code")
                subtitle: Translation.tr("Valorant format")
                fieldWidth: 220
                text: Config.options.crosshair.code
                onEdited: value => Config.options.crosshair.code = value
                RippleButton {
                    implicitWidth: 36
                    implicitHeight: 36
                    buttonRadius: Appearance.rounding.full
                    onClicked: Qt.openUrlExternally(`https://www.vcrdb.net/builder?c=${Config.options.crosshair.code}`)
                    contentItem: MaterialSymbol {
                        anchors.centerIn: parent
                        horizontalAlignment: Text.AlignHCenter
                        text: "open_in_new"
                        iconSize: 20
                    }
                    StyledToolTip {
                        text: Translation.tr("Open editor") + " (www.vcrdb.net)"
                    }
                }
            }
            SettingsTextRow {
                title: Translation.tr("Floating image")
                subtitle: Translation.tr("Image source")
                text: Config.options.overlay.floatingImage.imageSource
                onEdited: value => Config.options.overlay.floatingImage.imageSource = value
            }
        }
    }

    SettingsTab {
        tabIndex: 5
        tabName: Translation.tr("Screenshot")
        visible: page.currentTab === tabIndex

        SettingsGroup {
            title: Translation.tr("Highlight regions")
            SettingsPair {
                SettingsSwitchRow {
                    title: Translation.tr("Windows")
                    checked: Config.options.regionSelector.targetRegions.windows
                    onUserToggled: value => Config.options.regionSelector.targetRegions.windows = value
                }
                SettingsSwitchRow {
                    title: Translation.tr("Layers")
                    checked: Config.options.regionSelector.targetRegions.layers
                    onUserToggled: value => Config.options.regionSelector.targetRegions.layers = value
                }
            }
            SettingsSwitchRow {
                title: Translation.tr("Content")
                subtitle: Translation.tr("Images and boxed parts of the screen, may be inaccurate")
                checked: Config.options.regionSelector.targetRegions.content
                onUserToggled: value => Config.options.regionSelector.targetRegions.content = value
            }
        }
        SettingsGroup {
            title: Translation.tr("Selection")
            SettingsSegmentedRow {
                title: Translation.tr("Google Lens")
                currentValue: Config.options.search.imageSearch.useCircleSelection ? "circle" : "rectangles"
                onSelected: value => Config.options.search.imageSearch.useCircleSelection = (value === "circle")
                options: [
                    { label: Translation.tr("Rectangle"), value: "rectangles" },
                    { label: Translation.tr("Circle to Search"), value: "circle" }
                ]
            }
            SettingsSwitchRow {
                title: Translation.tr("Show aim lines")
                checked: Config.options.regionSelector.rect.showAimLines
                onUserToggled: value => Config.options.regionSelector.rect.showAimLines = value
            }
            SettingsSpinRow {
                title: Translation.tr("Circle stroke width")
                value: Config.options.regionSelector.circle.strokeWidth
                from: 1
                to: 20
                unit: Translation.tr("px")
                onUserChanged: value => Config.options.regionSelector.circle.strokeWidth = value
            }
            SettingsSpinRow {
                title: Translation.tr("Circle padding")
                value: Config.options.regionSelector.circle.padding
                from: 0
                to: 100
                stepSize: 5
                unit: Translation.tr("px")
                onUserChanged: value => Config.options.regionSelector.circle.padding = value
            }
        }
    }

    SettingsTab {
        tabIndex: 6
        tabName: Translation.tr("Cheat sheet")
        visible: page.currentTab === tabIndex

        SettingsGroup {
            title: Translation.tr("Cheat sheet")
            SettingsCard {
                StyledText {
                    text: Translation.tr("Super key symbol")
                    color: Appearance.colors.colOnLayer2
                    font.pixelSize: Appearance.font.pixelSize.small
                    font.weight: Font.Medium
                }
                Flow {
                    Layout.fillWidth: true
                    spacing: 2
                    Repeater {
                        // Use a nerdfont to see the icons
                        model: ["󰖳", "", "󰨡", "", "󰌽", "󰣇", "", "", "", "", "", "󱄛", "", "", "", "⌘", "󰀲", "󰟍", ""]
                        delegate: RippleButton {
                            id: keyButton
                            required property string modelData
                            implicitWidth: 40
                            implicitHeight: 40
                            buttonRadius: Appearance.rounding.full
                            toggled: Config.options.cheatsheet.superKey === keyButton.modelData
                            onClicked: Config.options.cheatsheet.superKey = keyButton.modelData
                            contentItem: StyledText {
                                anchors.centerIn: parent
                                horizontalAlignment: Text.AlignHCenter
                                text: keyButton.modelData
                                font.pixelSize: Appearance.font.pixelSize.large
                                color: keyButton.toggled ? Appearance.colors.colOnPrimary : Appearance.colors.colOnLayer2
                            }
                        }
                    }
                }
            }
            SettingsSwitchRow {
                title: Translation.tr("macOS-like modifier symbols")
                subtitle: Translation.tr("e.g. 󰘴 for Ctrl, 󰘵 for Alt, 󰘶 for Shift")
                checked: Config.options.cheatsheet.useMacSymbol
                onUserToggled: value => Config.options.cheatsheet.useMacSymbol = value
            }
            SettingsSwitchRow {
                title: Translation.tr("Symbols for function keys")
                subtitle: Translation.tr("e.g. 󱊫 for F1, 󱊶 for F12")
                checked: Config.options.cheatsheet.useFnSymbol
                onUserToggled: value => Config.options.cheatsheet.useFnSymbol = value
            }
            SettingsSwitchRow {
                title: Translation.tr("Symbols for mouse")
                checked: Config.options.cheatsheet.useMouseSymbol
                onUserToggled: value => Config.options.cheatsheet.useMouseSymbol = value
            }
            SettingsSwitchRow {
                title: Translation.tr("Split buttons")
                subtitle: Translation.tr("\"Ctrl + A\" instead of \"Ctrl A\"")
                checked: Config.options.cheatsheet.splitButtons
                onUserToggled: value => Config.options.cheatsheet.splitButtons = value
            }
            SettingsPair {
                SettingsSpinRow {
                    title: Translation.tr("Key size")
                    value: Config.options.cheatsheet.fontSize.key
                    from: 8
                    to: 30
                    onUserChanged: value => Config.options.cheatsheet.fontSize.key = value
                }
                SettingsSpinRow {
                    title: Translation.tr("Description size")
                    value: Config.options.cheatsheet.fontSize.comment
                    from: 8
                    to: 30
                    onUserChanged: value => Config.options.cheatsheet.fontSize.comment = value
                }
            }
        }
    }

    SettingsTab {
        tabIndex: 7
        tabName: Translation.tr("Fonts")
        visible: page.currentTab === tabIndex

        SettingsGroup {
            title: Translation.tr("Fonts")
            SettingsTextRow {
                title: Translation.tr("Main font")
                fieldWidth: 240
                placeholderText: "Google Sans Flex"
                text: Config.options.appearance.fonts.main
                onEdited: value => Config.options.appearance.fonts.main = value
            }
            SettingsTextRow {
                title: Translation.tr("Numbers font")
                fieldWidth: 240
                placeholderText: ""
                text: Config.options.appearance.fonts.numbers
                onEdited: value => Config.options.appearance.fonts.numbers = value
            }
            SettingsTextRow {
                title: Translation.tr("Title font")
                fieldWidth: 240
                placeholderText: ""
                text: Config.options.appearance.fonts.title
                onEdited: value => Config.options.appearance.fonts.title = value
            }
            SettingsTextRow {
                title: Translation.tr("Monospace font")
                fieldWidth: 240
                placeholderText: "JetBrains Mono NF"
                text: Config.options.appearance.fonts.monospace
                onEdited: value => Config.options.appearance.fonts.monospace = value
            }
            SettingsTextRow {
                title: Translation.tr("Nerd font icons")
                fieldWidth: 240
                placeholderText: "JetBrains Mono NF"
                text: Config.options.appearance.fonts.iconNerd
                onEdited: value => Config.options.appearance.fonts.iconNerd = value
            }
            SettingsTextRow {
                title: Translation.tr("Reading font")
                fieldWidth: 240
                placeholderText: "Readex Pro"
                text: Config.options.appearance.fonts.reading
                onEdited: value => Config.options.appearance.fonts.reading = value
            }
            SettingsTextRow {
                title: Translation.tr("Expressive font")
                fieldWidth: 240
                placeholderText: "Space Grotesk"
                text: Config.options.appearance.fonts.expressive
                onEdited: value => Config.options.appearance.fonts.expressive = value
            }
        }
    }
}
