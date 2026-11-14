import QtQuick
import QtQuick.Layouts
import ri.services
import ri.modules.common
import ri.modules.common.widgets

SettingsPage {
    id: page

    readonly property var clock: Config.options.background.widgets.clock
    readonly property var wallpick: Config.options.background.wallpick
    function phrases(text) {
        return text.split(";").map(p => p.trim()).filter(p => p.length > 0);
    }
    function stylePresent(styleName) {
        if (!page.clock.showOnlyWhenLocked && page.clock.style === styleName)
            return true;
        return page.clock.styleLocked === styleName;
    }
    readonly property bool clockOn: page.clock.enable
    readonly property bool digitalPresent: page.clockOn && stylePresent("digital")
    readonly property bool cookiePresent: page.clockOn && stylePresent("cookie")

    component ClockTile: SettingsPreviewTile {
        id: clockTile
        required property string styleName
        tileWidth: 150
        tileHeight: 84
        selected: page.clock.style === clockTile.styleName
        onClicked: Config.options.background.widgets.clock.style = clockTile.styleName
        SettingsWallpaperThumb {
            radius: Appearance.rounding.small
        }
        StyledText {
            visible: clockTile.styleName === "digital"
            anchors.centerIn: parent
            text: "12:47"
            color: "white"
            font.pixelSize: 30
            font.weight: Font.Bold
        }
        Rectangle {
            visible: clockTile.styleName === "cookie"
            anchors.centerIn: parent
            width: 50
            height: 50
            radius: 25
            color: Appearance.colors.colPrimaryContainer
            MaterialSymbol {
                anchors.centerIn: parent
                text: "schedule"
                iconSize: 34
                color: Appearance.colors.colOnPrimaryContainer
            }
        }
    }

    readonly property var placementOptions: [
        { label: Translation.tr("Draggable"), value: "free" },
        { label: Translation.tr("Least busy"), value: "leastBusy" },
        { label: Translation.tr("Most busy"), value: "mostBusy" }
    ]

    SettingsGroup {
        title: Translation.tr("Clock")

        SettingsSwitchRow {
            id: clockRow
            title: Translation.tr("Show clock")
            checked: page.clock.enable
            onUserToggled: value => Config.options.background.widgets.clock.enable = value
        }
        SettingsCard {
            shown: page.clockOn && !page.clock.showOnlyWhenLocked
            StyledText {
                text: Translation.tr("Style")
                color: Appearance.colors.colOnLayer2
                font.pixelSize: Appearance.font.pixelSize.small
                font.weight: Font.Medium
            }
            RowLayout {
                spacing: 4
                ClockTile { styleName: "digital"; label: Translation.tr("Digital") }
                ClockTile { styleName: "cookie"; label: Translation.tr("Cookie") }
            }
        }
        SettingsSegmentedRow {
            parentRow: clockRow
            shown: page.clockOn
            title: Translation.tr("Style on the lock screen")
            currentValue: page.clock.styleLocked
            onSelected: value => Config.options.background.widgets.clock.styleLocked = value
            options: [
                { label: Translation.tr("Digital"), value: "digital" },
                { label: Translation.tr("Cookie"), value: "cookie" }
            ]
        }
        SettingsSegmentedRow {
            parentRow: clockRow
            shown: page.clockOn
            title: Translation.tr("Placement")
            currentValue: page.clock.placementStrategy
            onSelected: value => Config.options.background.widgets.clock.placementStrategy = value
            options: page.placementOptions
        }

        // Cookie clock
        SettingsSegmentedRow {
            parentRow: clockRow
            shown: page.cookiePresent
            title: Translation.tr("Dial")
            currentValue: page.clock.cookie.dialNumberStyle
            onSelected: value => {
                Config.options.background.widgets.clock.cookie.dialNumberStyle = value;
                if (value !== "dots" && value !== "full")
                    Config.options.background.widgets.clock.cookie.hourMarks = false;
                if (value === "numbers")
                    Config.options.background.widgets.clock.cookie.timeIndicators = false;
            }
            options: [
                { label: Translation.tr("Numbers"), value: "numbers" },
                { label: Translation.tr("Dots"), value: "dots" },
                { label: Translation.tr("Full"), value: "full" },
                { label: Translation.tr("None"), value: "none" }
            ]
        }
        SettingsSpinRow {
            parentRow: clockRow
            shown: page.cookiePresent
            title: Translation.tr("Sides")
            value: page.clock.cookie.sides
            from: 0
            to: 40
            onUserChanged: value => Config.options.background.widgets.clock.cookie.sides = value
        }
        SettingsExpandRow {
            id: cookieMoreRow
            parentRow: clockRow
            shown: page.cookiePresent
            title: Translation.tr("Hands, date style, rotation")
        }
        SettingsSegmentedRow {
            parentRow: cookieMoreRow
            shown: page.cookiePresent && cookieMoreRow.expanded
            title: Translation.tr("Hour hand")
            currentValue: page.clock.cookie.hourHandStyle
            onSelected: value => Config.options.background.widgets.clock.cookie.hourHandStyle = value
            options: [
                { label: Translation.tr("Hide"), value: "hide" },
                { label: Translation.tr("Classic"), value: "classic" },
                { label: Translation.tr("Hollow"), value: "hollow" },
                { label: Translation.tr("Fill"), value: "fill" }
            ]
        }
        SettingsSegmentedRow {
            parentRow: cookieMoreRow
            shown: page.cookiePresent && cookieMoreRow.expanded
            title: Translation.tr("Minute hand")
            currentValue: page.clock.cookie.minuteHandStyle
            onSelected: value => Config.options.background.widgets.clock.cookie.minuteHandStyle = value
            options: [
                { label: Translation.tr("Hide"), value: "hide" },
                { label: Translation.tr("Classic"), value: "classic" },
                { label: Translation.tr("Thin"), value: "thin" },
                { label: Translation.tr("Medium"), value: "medium" },
                { label: Translation.tr("Bold"), value: "bold" }
            ]
        }
        SettingsSegmentedRow {
            parentRow: cookieMoreRow
            shown: page.cookiePresent && cookieMoreRow.expanded && Config.options.time.secondPrecision
            title: Translation.tr("Second hand")
            currentValue: page.clock.cookie.secondHandStyle
            onSelected: value => Config.options.background.widgets.clock.cookie.secondHandStyle = value
            options: [
                { label: Translation.tr("Hide"), value: "hide" },
                { label: Translation.tr("Classic"), value: "classic" },
                { label: Translation.tr("Line"), value: "line" },
                { label: Translation.tr("Dot"), value: "dot" }
            ]
        }
        SettingsSegmentedRow {
            parentRow: cookieMoreRow
            shown: page.cookiePresent && cookieMoreRow.expanded
            title: Translation.tr("Date style")
            currentValue: page.clock.cookie.dateStyle
            onSelected: value => Config.options.background.widgets.clock.cookie.dateStyle = value
            options: [
                { label: Translation.tr("Hide"), value: "hide" },
                { label: Translation.tr("Bubble"), value: "bubble" },
                { label: Translation.tr("Border"), value: "border" },
                { label: Translation.tr("Rect"), value: "rect" }
            ]
        }
        SettingsSwitchRow {
            parentRow: cookieMoreRow
            shown: page.cookiePresent && cookieMoreRow.expanded && (page.clock.cookie.dialNumberStyle === "dots" || page.clock.cookie.dialNumberStyle === "full")
            title: Translation.tr("Hour marks")
            checked: page.clock.cookie.hourMarks
            onUserToggled: value => Config.options.background.widgets.clock.cookie.hourMarks = value
        }
        SettingsSwitchRow {
            parentRow: cookieMoreRow
            shown: page.cookiePresent && cookieMoreRow.expanded && page.clock.cookie.dialNumberStyle !== "numbers"
            title: Translation.tr("Digits in the middle")
            checked: page.clock.cookie.timeIndicators
            onUserToggled: value => Config.options.background.widgets.clock.cookie.timeIndicators = value
        }
        SettingsSwitchRow {
            parentRow: cookieMoreRow
            shown: page.cookiePresent && cookieMoreRow.expanded
            title: Translation.tr("Constantly rotate")
            subtitle: Translation.tr("Very heavy on the GPU")
            checked: page.clock.cookie.constantlyRotate
            onUserToggled: value => Config.options.background.widgets.clock.cookie.constantlyRotate = value
        }
        SettingsSwitchRow {
            parentRow: cookieMoreRow
            shown: page.cookiePresent && cookieMoreRow.expanded
            title: Translation.tr("Old sine wave cookie")
            subtitle: Translation.tr("Softer look, less impressive morphing")
            checked: page.clock.cookie.useSineCookie
            onUserToggled: value => Config.options.background.widgets.clock.cookie.useSineCookie = value
        }

        // Digital clock
        SettingsPair {
            shown: page.digitalPresent
            SettingsSwitchRow {
                parentRow: clockRow
                title: Translation.tr("Vertical")
                checked: page.clock.digital.vertical
                onUserToggled: value => Config.options.background.widgets.clock.digital.vertical = value
            }
            SettingsSwitchRow {
                parentRow: clockRow
                title: Translation.tr("Show date")
                checked: page.clock.digital.showDate
                onUserToggled: value => Config.options.background.widgets.clock.digital.showDate = value
            }
        }
        SettingsSwitchRow {
            parentRow: clockRow
            shown: page.digitalPresent
            title: Translation.tr("Animate time change")
            checked: page.clock.digital.animateChange
            onUserToggled: value => Config.options.background.widgets.clock.digital.animateChange = value
        }
        SettingsSwitchRow {
            parentRow: clockRow
            shown: page.digitalPresent
            title: Translation.tr("Adaptive alignment")
            subtitle: Translation.tr("Aligns date and quote to where the clock is on screen")
            checked: page.clock.digital.adaptiveAlignment
            onUserToggled: value => Config.options.background.widgets.clock.digital.adaptiveAlignment = value
        }
        SettingsExpandRow {
            id: digitalFontRow
            parentRow: clockRow
            shown: page.digitalPresent
            title: Translation.tr("Clock font")
            subtitle: page.clock.digital.font.family
        }
        SettingsTextRow {
            parentRow: digitalFontRow
            shown: page.digitalPresent && digitalFontRow.expanded
            title: Translation.tr("Font family")
            fieldWidth: 220
            text: page.clock.digital.font.family
            onEdited: value => Config.options.background.widgets.clock.digital.font.family = value
        }
        SettingsSpinRow {
            parentRow: digitalFontRow
            shown: page.digitalPresent && digitalFontRow.expanded
            title: Translation.tr("Font weight")
            value: page.clock.digital.font.weight
            from: 1
            to: 1000
            stepSize: 50
            onUserChanged: value => Config.options.background.widgets.clock.digital.font.weight = value
        }
        SettingsSpinRow {
            parentRow: digitalFontRow
            shown: page.digitalPresent && digitalFontRow.expanded
            title: Translation.tr("Font size")
            value: page.clock.digital.font.size
            from: 50
            to: 700
            stepSize: 10
            onUserChanged: value => Config.options.background.widgets.clock.digital.font.size = value
        }
        SettingsSpinRow {
            parentRow: digitalFontRow
            shown: page.digitalPresent && digitalFontRow.expanded
            title: Translation.tr("Font width")
            value: page.clock.digital.font.width
            from: 25
            to: 125
            stepSize: 5
            onUserChanged: value => Config.options.background.widgets.clock.digital.font.width = value
        }
        SettingsSpinRow {
            parentRow: digitalFontRow
            shown: page.digitalPresent && digitalFontRow.expanded
            title: Translation.tr("Font roundness")
            value: page.clock.digital.font.roundness
            from: 0
            to: 100
            stepSize: 5
            onUserChanged: value => Config.options.background.widgets.clock.digital.font.roundness = value
        }

        SettingsSwitchRow {
            parentRow: clockRow
            shown: page.clockOn
            title: Translation.tr("Show only on the lock screen")
            checked: page.clock.showOnlyWhenLocked
            onUserToggled: value => Config.options.background.widgets.clock.showOnlyWhenLocked = value
        }
    }

    SettingsGroup {
        title: Translation.tr("Under the clock")

        SettingsSwitchRow {
            id: weatherRow
            title: Translation.tr("Weather")
            checked: Config.options.background.widgets.weather.enable
            onUserToggled: value => Config.options.background.widgets.weather.enable = value
        }
        SettingsSegmentedRow {
            parentRow: weatherRow
            shown: Config.options.background.widgets.weather.enable
            title: Translation.tr("Weather placement")
            currentValue: Config.options.background.widgets.weather.placementStrategy
            onSelected: value => Config.options.background.widgets.weather.placementStrategy = value
            options: page.placementOptions
        }
        SettingsSwitchRow {
            id: quoteRow
            title: Translation.tr("Quote")
            checked: page.clock.quote.enable
            onUserToggled: value => Config.options.background.widgets.clock.quote.enable = value
        }
        SettingsTextRow {
            parentRow: quoteRow
            shown: page.clock.quote.enable
            title: Translation.tr("Quote text")
            text: page.clock.quote.text
            onEdited: value => Config.options.background.widgets.clock.quote.text = value
        }
    }

    SettingsGroup {
        title: Translation.tr("Wallpaper selector")
        SettingsSwitchRow {
            title: Translation.tr("Use system file picker")
            keywords: "wallpaper selector dialog"
            checked: Config.options.wallpaperSelector.useSystemFileDialog
            onUserToggled: value => Config.options.wallpaperSelector.useSystemFileDialog = value
        }
    }

    SettingsGroup {
        title: Translation.tr("Wallpaper motion")

        SettingsSwitchRow {
            id: parallaxRow
            title: Translation.tr("Parallax")
            subtitle: Translation.tr("Wallpaper slides when you switch workspaces")
            checked: Config.options.background.parallax.enableWorkspace
            onUserToggled: value => Config.options.background.parallax.enableWorkspace = value
        }
        SettingsSwitchRow {
            parentRow: parallaxRow
            shown: Config.options.background.parallax.enableWorkspace
            title: Translation.tr("Vertical")
            checked: Config.options.background.parallax.vertical
            onUserToggled: value => Config.options.background.parallax.vertical = value
        }
        SettingsSwitchRow {
            title: Translation.tr("Slide with sidebars")
            checked: Config.options.background.parallax.enableSidebar
            onUserToggled: value => Config.options.background.parallax.enableSidebar = value
        }
        SettingsSpinRow {
            shown: Config.options.background.parallax.enableWorkspace || Config.options.background.parallax.enableSidebar
            title: Translation.tr("Wallpaper zoom")
            value: Math.round(Config.options.background.parallax.workspaceZoom * 100)
            from: 10
            to: 200
            stepSize: 5
            unit: "%"
            onUserChanged: value => Config.options.background.parallax.workspaceZoom = value / 100
        }
    }

    SettingsGroup {
        title: Translation.tr("Wallpaper picker")

        SettingsSegmentedRow {
            title: Translation.tr("Source")
            currentValue: page.wallpick.sources.join("+")
            onSelected: value => Config.options.background.wallpick.sources = value.split("+")
            options: [
                { label: "Wallhaven", value: "wallhaven" },
                { label: "Konachan", value: "konachan" },
                { label: Translation.tr("Both"), value: "wallhaven+konachan" }
            ]
        }
        SettingsTextRow {
            title: Translation.tr("I like")
            subtitle: Translation.tr("Short descriptions, separated by semicolons")
            text: page.wallpick.like.join("; ")
            onEdited: value => Config.options.background.wallpick.like = page.phrases(value)
        }
        SettingsTextRow {
            title: Translation.tr("I don't like")
            text: page.wallpick.dislike.join("; ")
            onEdited: value => Config.options.background.wallpick.dislike = page.phrases(value)
        }
        SettingsSwitchRow {
            title: Translation.tr("Whole scenes only")
            subtitle: Translation.tr("Skip a lone object on a plain background")
            checked: page.wallpick.fullScene
            onUserToggled: value => Config.options.background.wallpick.fullScene = value
        }
        SettingsSwitchRow {
            title: Translation.tr("Subject to one side")
            subtitle: Translation.tr("Leaves room for the clock and widgets")
            checked: page.wallpick.subjectSide
            onUserToggled: value => Config.options.background.wallpick.subjectSide = value
        }
        SettingsSwitchRow {
            id: wallpickAccentRow
            title: Translation.tr("Accent from the wallpaper")
            checked: page.wallpick.pickAccent
            onUserToggled: value => Config.options.background.wallpick.pickAccent = value
        }
        SettingsTextRow {
            parentRow: wallpickAccentRow
            shown: page.wallpick.pickAccent
            title: Translation.tr("Preferred accents")
            text: page.wallpick.accentLike.join("; ")
            onEdited: value => Config.options.background.wallpick.accentLike = page.phrases(value)
        }
        SettingsTextRow {
            parentRow: wallpickAccentRow
            shown: page.wallpick.pickAccent
            title: Translation.tr("Avoided accents")
            text: page.wallpick.accentDislike.join("; ")
            onEdited: value => Config.options.background.wallpick.accentDislike = page.phrases(value)
        }
        SettingsSpinRow {
            title: Translation.tr("Candidates per pick")
            subtitle: Translation.tr("More finds a better match but takes longer")
            value: page.wallpick.candidates
            from: 8
            to: 120
            stepSize: 8
            onUserChanged: value => Config.options.background.wallpick.candidates = value
        }
        SettingsSpinRow {
            title: Translation.tr("Variety")
            value: Math.round(page.wallpick.randomness * 100)
            from: 0
            to: 100
            stepSize: 5
            unit: "%"
            onUserChanged: value => Config.options.background.wallpick.randomness = value / 100
        }
        SettingsTextRow {
            title: Translation.tr("Layout wish")
            subtitle: Translation.tr("For example: clock on the left, music in the middle")
            text: page.wallpick.layoutWish
            onEdited: value => Config.options.background.wallpick.layoutWish = value
        }
        SettingsRow {
            title: Translation.tr("Arrange the bar and clock")
            subtitle: Wallpick.busy ? Wallpick.status : Translation.tr("Uses the layout wish and the current wallpaper")
            RippleButtonWithIcon {
                enabled: !Wallpick.busy
                materialIcon: "dashboard_customize"
                mainText: Translation.tr("Arrange")
                buttonRadius: Appearance.rounding.full
                colBackground: Appearance.colors.colLayer3
                colBackgroundHover: Appearance.colors.colLayer3Hover
                onClicked: Wallpick.restyle()
            }
        }
        SettingsRow {
            title: Translation.tr("Forget likes and dislikes")
            RippleButtonWithIcon {
                enabled: !Wallpick.busy
                materialIcon: "restart_alt"
                mainText: Translation.tr("Reset")
                buttonRadius: Appearance.rounding.full
                colBackground: Appearance.colors.colLayer3
                colBackgroundHover: Appearance.colors.colLayer3Hover
                onClicked: Wallpick.resetTaste()
            }
        }
    }
}
