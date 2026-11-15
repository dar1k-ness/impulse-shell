pragma Singleton
pragma ComponentBehavior: Bound

import ri.modules.common
import QtQuick
import Rustshell
import Impulse
import Rustshell.Io
import Rustshell.Hyprland

/**
 * Automatically reloads generated material colors.
 * It is necessary to run reapplyTheme() on startup because Singletons are lazily loaded.
 */
Singleton {
    id: root
    property string filePath: Directories.generatedMaterialThemePath

    function reapplyTheme() {
        themeFileView.reload()
    }

    property bool loadedOnce: false
    // Each read of the colors file, applied or failed, after its colors (and darkmode) are set
    signal themeRead()

    function applyColors(fileContent) {
        const json = JSON.parse(fileContent)
        const colors = {}
        let changed = false
        for (const key in json) {
            if (json.hasOwnProperty(key)) {
                // Convert snake_case to CamelCase
                const camelCaseKey = key.replace(/_([a-z])/g, (g) => g[1].toUpperCase())
                const m3Key = `m3${camelCaseKey}`
                colors[m3Key] = json[key]
                if (String(Appearance.m3colors[m3Key]) !== String(Qt.color(json[key])))
                    changed = true
            }
        }
        if (root.loadedOnce && !changed)
            return
        // A new scheme (light/dark, accent) fades in over the old one: impulse-shell crossfades each
        // window from its last frame. The first load at startup applies at once.
        if (root.loadedOnce)
            Crossfade.request(400)
        root.loadedOnce = true
        for (const k in colors)
            Appearance.m3colors[k] = colors[k]
        
        Appearance.m3colors.darkmode = (Appearance.m3colors.m3background.hslLightness < 0.5)
    }

    function resetFilePathNextTime() {
        resetFilePathNextWallpaperChange.enabled = true
    }

    Connections {
        id: resetFilePathNextWallpaperChange
        enabled: false
        target: Config.options.background
        function onWallpaperPathChanged() {
            root.filePath = ""
            root.filePath = Directories.generatedMaterialThemePath
            resetFilePathNextWallpaperChange.enabled = false
        }
    }

	FileView { 
        id: themeFileView
        path: Qt.resolvedUrl(root.filePath)
        watchChanges: true
        onFileChanged: this.reload()
        // Every finished (re)load applies at once; a half-written file is skipped, its next
        // change brings the rest.
        onLoaded: {
            try {
                root.applyColors(themeFileView.text())
            } catch (e) {
                console.warn("[MaterialThemeLoader] Incomplete colors file:", e)
            }
            root.themeRead()
        }
        onLoadFailed: {
            root.resetFilePathNextTime();
            root.themeRead();
        }
    }

    function toggleLightDark() {
        const currentlyDark = Appearance.m3colors.darkmode;
        Switchwall.run({ mode: currentlyDark ? "light" : "dark", noSwitch: true });
    }

    GlobalShortcut {
        name: "toggleLightDark"
        description: "Toggles between dark theme and light theme"

        onPressed: {
            root.toggleLightDark();
        }
    }

    IpcHandler {
        target: "theme"

        function toggleLightDark(): void {
            root.toggleLightDark();
        }
    }
}
