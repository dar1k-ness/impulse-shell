pragma Singleton

import ri.modules.common
import ri.modules.common.functions
import Rustshell
import Rustshell.Io
import Impulse

Singleton {
    id: root
    property string firstRunFilePath: `${Directories.state}/user/first_run.txt`
    property string firstRunFileContent: "This file is just here to confirm you've been greeted :>"
    property string firstRunNotifSummary: "Welcome!"
    property string firstRunNotifBody: "Hit Super+/ for a list of keybinds"
    property string defaultWallpaperPath: FileUtils.trimFileProtocol(`${Directories.assetsPath}/images/default_wallpaper.png`)
    property string welcomeQmlPath: FileUtils.trimFileProtocol(Rustshell.shellPath("welcome.qml"))

    function load() {
        firstRunFileView.reload()
    }

    function enableNextTime() {
        Files.remove(FileUtils.trimFileProtocol(root.firstRunFilePath))
    }
    function disableNextTime() {
        Files.writeLine(FileUtils.trimFileProtocol(root.firstRunFilePath), root.firstRunFileContent)
    }

    function handleFirstRun() {
        Switchwall.run({ image: root.defaultWallpaperPath })
        ShellControl.openWindow(root.welcomeQmlPath)
    }

    FileView {
        id: firstRunFileView
        path: Qt.resolvedUrl(firstRunFilePath)
        onLoadFailed: (error) => {
            if (error == FileViewError.FileNotFound) {
                firstRunFileView.setText(root.firstRunFileContent)
                root.handleFirstRun()
            }
        }
    }
}
