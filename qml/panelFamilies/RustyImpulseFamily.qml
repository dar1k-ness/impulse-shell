import QtQuick
import Rustshell

import ri.modules.common
import ri.services
import ri.modules.rusty.background
import ri.modules.rusty.bar
import ri.modules.rusty.cheatsheet
import ri.modules.rusty.dock
import ri.modules.rusty.lock
import ri.modules.rusty.mediaControls
import ri.modules.rusty.notificationPopup
import ri.modules.rusty.onScreenDisplay
import ri.modules.rusty.onScreenKeyboard
import ri.modules.rusty.overview
import ri.modules.rusty.polkit
import ri.modules.rusty.regionSelector
import ri.modules.rusty.screenCorners
import ri.modules.rusty.screenTranslator
import ri.modules.rusty.sessionScreen
import ri.modules.rusty.sidebarLeft
import ri.modules.rusty.sidebarRight
import ri.modules.rusty.overlay
import ri.modules.rusty.verticalBar
import ri.modules.rusty.wallpaperSelector

Scope {
    PanelLoader { extraCondition: !Config.options.bar.vertical || LayoutState.hasContainer("panel"); component: Bar {} }
    PanelLoader { component: Background {} }
    // Layout edit mode (rusty-impulse: a layout with containers; the file imports Impulse).
    LazyLoader { active: Config.ready && LayoutState.containerKinds.length > 0; source: Qt.resolvedUrl("../modules/rusty/editMode/EditMode.qml") }
    // The style review opens the edit mode once the wallpaper picker has put a style on the screen.
    Component.onCompleted: StyleReview.load()
    PanelLoader { component: Cheatsheet {} }
    PanelLoader { extraCondition: Config.options.dock.enable; component: Dock {} }
    PanelLoader { component: Lock {} }
    PanelLoader { component: MediaControls {} }
    PanelLoader { component: NotificationPopup {} }
    PanelLoader { component: OnScreenDisplay {} }
    PanelLoader { component: OnScreenKeyboard {} }
    PanelLoader { component: Overlay {} }
    PanelLoader { component: Overview {} }
    PanelLoader { component: Polkit {} }
    PanelLoader { component: RegionSelector {} }
    PanelLoader { component: ScreenCorners {} }
    PanelLoader { component: ScreenTranslator {} }
    PanelLoader { component: SessionScreen {} }
    PanelLoader { component: SidebarLeft {} }
    PanelLoader { component: SidebarRight {} }
    PanelLoader { extraCondition: Config.options.bar.vertical || LayoutState.hasContainer("panel"); component: VerticalBar {} }
    PanelLoader { component: WallpaperSelector {} }
}
