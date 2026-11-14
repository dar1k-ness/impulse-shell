//@ pragma UseQApplication
//@ pragma Env QS_NO_RELOAD_POPUP=1
//@ pragma Env QT_QUICK_CONTROLS_STYLE=Basic
//@ pragma Env QT_QUICK_FLICKABLE_WHEEL_DECELERATION=10000

// Adjust this to make the app smaller or larger
//@ pragma Env QT_SCALE_FACTOR=1

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Window
import Rustshell
import Impulse
import ri.services
import ri.modules.common
import ri.modules.common.widgets
import ri.modules.common.functions as CF

ApplicationWindow {
    id: root
    property real contentPadding: 8
    property var pages: [
        {
            name: Translation.tr("Quick"),
            icon: "instant_mix",
            component: "modules/settings/QuickConfig.qml"
        },
        {
            name: Translation.tr("General"),
            icon: "browse",
            component: "modules/settings/GeneralConfig.qml"
        },
        {
            name: Translation.tr("Bar"),
            icon: "toast",
            iconRotation: 180,
            component: "modules/settings/BarConfig.qml"
        },
        {
            name: Translation.tr("Background"),
            icon: "texture",
            component: "modules/settings/BackgroundConfig.qml"
        },
        {
            name: Translation.tr("Interface"),
            icon: "bottom_app_bar",
            component: "modules/settings/InterfaceConfig.qml"
        },
        {
            name: Translation.tr("Services"),
            icon: "settings",
            component: "modules/settings/ServicesConfig.qml"
        },
        {
            name: Translation.tr("Advanced"),
            icon: "construction",
            component: "modules/settings/AdvancedConfig.qml"
        },
        {
            name: Translation.tr("About"),
            icon: "info",
            component: "modules/settings/About.qml"
        }
    ]
    property int currentPage: 0
    property var searchResults: []
    property int searchIndex: 0

    // Page, inner tab and revealing row of a settings row, for search results.
    function locateSetting(row) {
        let page = null;
        let tab = null;
        let pageIndex = -1;
        for (let it = row.parent; it; it = it.parent) {
            if (tab === null && it.settingsTab === true)
                tab = it;
            if (page === null && it.settingsPage === true)
                page = it;
            if (it.pageIndex !== undefined) {
                pageIndex = it.pageIndex;
                break;
            }
        }
        let group = "";
        for (let it = row.parent; it; it = it.parent) {
            if (it.settingsGroup === true) {
                group = it.title;
                break;
            }
        }
        let path = pageIndex >= 0 ? root.pages[pageIndex].name : "";
        if (tab !== null)
            path += ` › ${tab.tabName}`;
        if (group.length > 0 && (tab === null || group !== tab.tabName))
            path += ` › ${group}`;
        return {
            page: page,
            tab: tab,
            pageIndex: pageIndex,
            path: path
        };
    }

    function goToSetting(row) {
        const where = root.locateSetting(row);
        if (where.pageIndex < 0)
            return;
        if (where.tab !== null && where.page !== null)
            where.page.currentTab = where.tab.tabIndex;
        let target = row;
        if (row.parentRow && row.parentRow.collapsed)
            row.parentRow.collapsed = false;
        if (!row.shown && row.parentRow) {
            if (row.parentRow.expanded !== undefined)
                row.parentRow.expanded = true;
            else
                target = row.parentRow;
        }
        searchInput.text = "";
        root.currentPage = where.pageIndex;
        // The page keeps the row in view while the switch and the reveal lay it out
        if (where.page)
            where.page.scrollToItem(target);
        target.highlight();
    }

    visible: true
    onClosing: Qt.quit()
    title: "rusty-impulse Settings"

    Component.onCompleted: {
        MaterialThemeLoader.reapplyTheme()
        Config.readWriteDelay = 0 // Settings app always only sets one var at a time so delay isn't needed
    }

    minimumWidth: 800
    minimumHeight: 520
    width: 1120
    height: 780
    color: Appearance.m3colors.m3background

    ColumnLayout {
        anchors {
            fill: parent
            margins: contentPadding
        }

        Keys.onPressed: (event) => {
            if (event.modifiers === Qt.ControlModifier) {
                if (event.key === Qt.Key_F) {
                    searchInput.forceActiveFocus();
                    event.accepted = true;
                }
                else if (event.key === Qt.Key_PageDown) {
                    root.currentPage = Math.min(root.currentPage + 1, root.pages.length - 1)
                    event.accepted = true;
                } 
                else if (event.key === Qt.Key_PageUp) {
                    root.currentPage = Math.max(root.currentPage - 1, 0)
                    event.accepted = true;
                }
                else if (event.key === Qt.Key_Tab) {
                    root.currentPage = (root.currentPage + 1) % root.pages.length;
                    event.accepted = true;
                }
                else if (event.key === Qt.Key_Backtab) {
                    root.currentPage = (root.currentPage - 1 + root.pages.length) % root.pages.length;
                    event.accepted = true;
                }
            }
        }

        Item { // Titlebar
            visible: Config.options?.windows.showTitlebar
            Layout.fillWidth: true
            Layout.fillHeight: false
            implicitHeight: Math.max(titleText.implicitHeight, windowControlsRow.implicitHeight)
            StyledText {
                id: titleText
                anchors {
                    left: Config.options.windows.centerTitle ? undefined : parent.left
                    horizontalCenter: Config.options.windows.centerTitle ? parent.horizontalCenter : undefined
                    verticalCenter: parent.verticalCenter
                    leftMargin: 12
                }
                color: Appearance.colors.colOnLayer0
                text: Translation.tr("Settings")
                font {
                    family: Appearance.font.family.title
                    pixelSize: Appearance.font.pixelSize.title
                    variableAxes: Appearance.font.variableAxes.title
                }
            }
            RowLayout { // Window controls row
                id: windowControlsRow
                anchors.verticalCenter: parent.verticalCenter
                anchors.right: parent.right
                RippleButton {
                    buttonRadius: Appearance.rounding.full
                    implicitWidth: 35
                    implicitHeight: 35
                    onClicked: root.close()
                    contentItem: MaterialSymbol {
                        anchors.centerIn: parent
                        horizontalAlignment: Text.AlignHCenter
                        text: "close"
                        iconSize: 20
                    }
                }
            }
        }

        RowLayout { // Window content with navigation rail and content pane
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: contentPadding
            Item {
                id: navRailWrapper
                Layout.fillHeight: true
                Layout.margins: 5
                // Sized by the tabs and the widest FAB label, so "Path copied" moves nothing;
                // collapsed, by the widest tab label under its icon
                implicitWidth: navRail.expanded ? Math.max(150, tabArray.implicitWidth, fab.reservedWidth) : navRail.collapsedWidth
                Behavior on implicitWidth {
                    animation: Appearance.animation.elementMoveFast.numberAnimation.createObject(this)
                }
                NavigationRail { // Window content with navigation rail and content pane
                    id: navRail
                    anchors {
                        left: parent.left
                        right: parent.right
                        top: parent.top
                        bottom: parent.bottom
                    }
                    spacing: 10
                    expanded: root.width > 900
                    collapsedWidth: Math.max(fab.baseSize, tabArray.collapsedWidth)
                    
                    NavigationRailExpandButton {
                        focus: root.visible
                    }

                    NavigationRailTabArray {
                        id: tabArray
                        currentIndex: root.currentPage
                        expanded: navRail.expanded
                        inset: navRail.inset
                        // Expanding, the tabs reveal with the widening column instead of running
                        // under the content (collapsed, the last label hangs below its button)
                        Layout.fillWidth: true
                        clip: navRail.expanded
                        Repeater {
                            model: root.pages
                            NavigationRailButton {
                                required property var index
                                required property var modelData
                                toggled: root.currentPage === index
                                onPressed: root.currentPage = index;
                                expanded: navRail.expanded
                                inset: navRail.inset
                                labelInset: navRail.restingInset
                                buttonIcon: modelData.icon
                                buttonIconRotation: modelData.iconRotation || 0
                                buttonText: modelData.name
                                showToggledHighlight: false
                            }
                        }
                    }

                    Item {
                        Layout.fillHeight: true
                    }

                    FloatingActionButton {
                        id: fab
                        property bool justCopied: false
                        iconText: justCopied ? "check" : "edit"
                        buttonText: justCopied ? Translation.tr("Path copied") : Translation.tr("Config file")
                        reservedText: Translation.tr("Path copied")
                        expanded: navRail.expanded
                        Layout.leftMargin: navRail.inset
                        downAction: () => {
                            Qt.openUrlExternally(`${Directories.config}/rusty-impulse/config.json`);
                        }
                        altAction: () => {
                            Rustshell.clipboardText = CF.FileUtils.trimFileProtocol(`${Directories.config}/rusty-impulse/config.json`);
                            fab.justCopied = true;
                            revertTextTimer.restart()
                        }

                        Timer {
                            id: revertTextTimer
                            interval: 1500
                            onTriggered: {
                                fab.justCopied = false;
                            }
                        }

                        StyledToolTip {
                            text: Translation.tr("Open the shell config file\nAlternatively right-click to copy path")
                        }
                    }
                }
            }
            Rectangle { // Content container
                Layout.fillWidth: true
                Layout.fillHeight: true
                color: Appearance.m3colors.m3surfaceContainerLow
                radius: Appearance.rounding.windowRounding - root.contentPadding

                Item { // Page title and settings search
                    id: header
                    z: 2
                    anchors {
                        top: parent.top
                        horizontalCenter: parent.horizontalCenter
                        topMargin: 22
                    }
                    width: Math.min(860, parent.width - 64)
                    height: 48

                    StyledText {
                        anchors {
                            left: parent.left
                            leftMargin: 4
                            verticalCenter: parent.verticalCenter
                        }
                        text: root.pages[root.currentPage].name
                        color: Appearance.colors.colOnLayer1
                        font {
                            family: Appearance.font.family.title
                            pixelSize: Appearance.font.pixelSize.title
                            variableAxes: Appearance.font.variableAxes.title
                        }
                    }

                    Rectangle {
                        id: searchBox
                        anchors {
                            right: parent.right
                            verticalCenter: parent.verticalCenter
                        }
                        width: Math.min(280, header.width * 0.48)
                        height: 44
                        radius: height / 2
                        color: searchInput.activeFocus ? Appearance.colors.colLayer2Hover : Appearance.colors.colLayer2
                        border.width: searchInput.activeFocus ? 2 : 0
                        border.color: Appearance.colors.colPrimary

                        MaterialSymbol {
                            id: searchIcon
                            anchors {
                                left: parent.left
                                leftMargin: 14
                                verticalCenter: parent.verticalCenter
                            }
                            text: "search"
                            iconSize: 22
                            color: Appearance.colors.colOnLayer2
                        }
                        StyledTextInput {
                            id: searchInput
                            anchors {
                                left: searchIcon.right
                                right: clearSearch.left
                                leftMargin: 10
                                rightMargin: 6
                                verticalCenter: parent.verticalCenter
                            }
                            clip: true
                            color: Appearance.colors.colOnLayer2
                            onTextChanged: {
                                root.searchResults = SettingsSearch.search(text);
                                root.searchIndex = 0;
                            }
                            Keys.onEscapePressed: searchInput.text = ""
                            Keys.onDownPressed: {
                                const n = Math.min(root.searchResults.length, 30);
                                if (n > 0)
                                    root.searchIndex = (root.searchIndex + 1) % n;
                            }
                            Keys.onUpPressed: {
                                const n = Math.min(root.searchResults.length, 30);
                                if (n > 0)
                                    root.searchIndex = (root.searchIndex + n - 1) % n;
                            }
                            Keys.onReturnPressed: {
                                if (root.searchResults.length > root.searchIndex)
                                    root.goToSetting(root.searchResults[root.searchIndex]);
                            }
                            Keys.onEnterPressed: {
                                if (root.searchResults.length > root.searchIndex)
                                    root.goToSetting(root.searchResults[root.searchIndex]);
                            }
                        }
                        StyledText {
                            anchors.fill: searchInput
                            verticalAlignment: Text.AlignVCenter
                            visible: searchInput.text.length === 0
                            text: Translation.tr("Search settings")
                            color: Appearance.colors.colSubtext
                            font.pixelSize: Appearance.font.pixelSize.small
                        }
                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.IBeamCursor
                            onClicked: searchInput.forceActiveFocus()
                        }
                        RippleButton {
                            id: clearSearch
                            visible: searchInput.text.length > 0
                            anchors {
                                right: parent.right
                                rightMargin: 6
                                verticalCenter: parent.verticalCenter
                            }
                            implicitWidth: visible ? 32 : 0
                            implicitHeight: 32
                            buttonRadius: Appearance.rounding.full
                            onClicked: searchInput.text = ""
                            contentItem: MaterialSymbol {
                                anchors.centerIn: parent
                                horizontalAlignment: Text.AlignHCenter
                                text: "close"
                                iconSize: 18
                            }
                        }
                    }

                    Rectangle { // Search results
                        visible: searchInput.text.trim().length > 0
                        anchors {
                            top: searchBox.bottom
                            topMargin: 6
                            right: searchBox.right
                        }
                        width: Math.max(searchBox.width, 340)
                        height: Math.min(resultsColumn.implicitHeight + 12, 380)
                        radius: Appearance.rounding.normal
                        color: Appearance.m3colors.m3surfaceContainerHigh
                        clip: true

                        StyledFlickable {
                            anchors.fill: parent
                            anchors.margins: 6
                            contentHeight: resultsColumn.implicitHeight
                            clip: true

                            ColumnLayout {
                                id: resultsColumn
                                width: parent.width
                                spacing: 0

                                StyledText {
                                    visible: root.searchResults.length === 0
                                    Layout.margins: 12
                                    text: Translation.tr("Nothing found")
                                    color: Appearance.colors.colSubtext
                                    font.pixelSize: Appearance.font.pixelSize.small
                                }

                                Repeater {
                                    model: root.searchResults.slice(0, 30)
                                    delegate: RippleButton {
                                        id: resultButton
                                        required property var modelData
                                        required property int index
                                        Layout.fillWidth: true
                                        implicitHeight: 52
                                        buttonRadius: Appearance.rounding.small
                                        colBackground: resultButton.index === root.searchIndex ? Appearance.colors.colSecondaryContainer : CF.ColorUtils.transparentize(Appearance.colors.colLayer1Hover, 1)
                                        onHoveredChanged: {
                                            if (resultButton.hovered)
                                                root.searchIndex = resultButton.index;
                                        }
                                        onClicked: root.goToSetting(resultButton.modelData)
                                        contentItem: ColumnLayout {
                                            anchors {
                                                left: parent.left
                                                right: parent.right
                                                verticalCenter: parent.verticalCenter
                                                leftMargin: 12
                                                rightMargin: 12
                                            }
                                            spacing: 0
                                            StyledText {
                                                Layout.fillWidth: true
                                                text: resultButton.modelData.title
                                                color: Appearance.colors.colOnLayer2
                                                font.pixelSize: Appearance.font.pixelSize.small
                                                elide: Text.ElideRight
                                            }
                                            StyledText {
                                                Layout.fillWidth: true
                                                text: root.locateSetting(resultButton.modelData).path
                                                color: Appearance.colors.colSubtext
                                                font.pixelSize: Appearance.font.pixelSize.smallie
                                                elide: Text.ElideRight
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Every page stays loaded once built, and pages not visited yet are built one
                // at a time while the window is idle: switching sections never instantiates a
                // page under the switch animation.
                Item {
                    id: pageStack
                    anchors {
                        left: parent.left
                        right: parent.right
                        bottom: parent.bottom
                        top: header.bottom
                        topMargin: 16
                    }
                    clip: true
                    property int shownPage: root.currentPage
                    onShownPageChanged: Crossfade.request(220)
                    property int preloadCount: 1
                    // The rest preload one page per event loop turn, each once the previous is built
                    function preloadNext() {
                        if (pageStack.preloadCount < root.pages.length)
                            pageStack.preloadCount += 1;
                    }

                    Repeater {
                        model: root.pages
                        Loader {
                            id: pageLoader
                            required property int index
                            required property var modelData
                            property bool shown: index === pageStack.shownPage
                            property bool requested: false
                            readonly property int pageIndex: index
                            anchors.fill: parent
                            // The new page slides in; the old one fades out over it (rusty-impulse crossfade)
                            property real shift: shown ? 0 : 14
                            visible: shown
                            enabled: shown
                            z: shown ? 1 : 0
                            transform: Translate {
                                y: pageLoader.shift
                            }
                            Behavior on shift {
                                NumberAnimation {
                                    duration: 260
                                    easing.type: Easing.OutCubic
                                }
                            }
                            active: Config.ready && (shown || requested || index < pageStack.preloadCount)
                            source: modelData.component
                            onShownChanged: if (shown) requested = true
                            Component.onCompleted: if (shown) requested = true
                            // A page that fails to load (Loader.Error) is done too: the next ones still preload
                            readonly property bool preloadFrontier: index === pageStack.preloadCount - 1 && (status === Loader.Ready || status === Loader.Error)
                            onPreloadFrontierChanged: if (preloadFrontier) Qt.callLater(pageStack.preloadNext)
                        }
                    }
                }
            }
        }
    }
}
