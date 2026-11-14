import QtQuick
import Rustshell

import ri.modules.common
import ri.modules.waffle.actionCenter
import ri.modules.waffle.background
import ri.modules.waffle.bar
import ri.modules.waffle.lock
import ri.modules.waffle.notificationCenter
import ri.modules.waffle.notificationPopup
import ri.modules.waffle.onScreenDisplay
// import ri.modules.waffle.overlay
import ri.modules.waffle.polkit
import ri.modules.waffle.screenSnip
import ri.modules.waffle.startMenu
import ri.modules.waffle.sessionScreen
import ri.modules.waffle.taskView

// Fallbacks
import ri.modules.rusty.cheatsheet
import ri.modules.rusty.onScreenKeyboard
import ri.modules.rusty.overlay
import ri.modules.rusty.screenTranslator
import ri.modules.rusty.wallpaperSelector

Scope {
    PanelLoader { component: WaffleActionCenter {} }
    PanelLoader { component: WaffleBar {} }
    PanelLoader { component: WaffleBackground {} }
    PanelLoader { component: WaffleLock {} }
    PanelLoader { component: WaffleNotificationCenter {} }
    PanelLoader { component: WaffleNotificationPopup {} }
    PanelLoader { component: WaffleOSD {} }
    // PanelLoader { component: WaffleOverlay {} }
    PanelLoader { component: WafflePolkit {} }
    PanelLoader { component: WScreenSnip {} }
    PanelLoader { component: WaffleStartMenu {} }
    PanelLoader { component: WaffleSessionScreen {} }
    PanelLoader { component: WaffleTaskView {} }

    PanelLoader { component: Cheatsheet {} }
    PanelLoader { component: OnScreenKeyboard {} }
    PanelLoader { component: Overlay {} }
    PanelLoader { component: ScreenTranslator {} }
    PanelLoader { component: WallpaperSelector {} }
}
