import QtQuick
import Rustshell
import Impulse
import ri.modules.common

/**
 * Captures a screen into `screenshotPath` while `running`, then emits `exited`
 * (exit code 0 when the file was written).
 */
Scope {
    id: screenshotProc
    property bool running: true
    property string screenshotDir: Directories.screenshotTemp
    required property ShellScreen screen
    property string screenshotPath: `${screenshotDir}/image-${screen.name}`

    signal exited(exitCode: int, exitStatus: int)

    property bool capturing: false
    function capture() {
        if (!running || capturing)
            return;
        capturing = true;
        Screenshot.capture(screen.name, screenshotPath);
    }
    onRunningChanged: capture()
    Component.onCompleted: capture()

    Connections {
        target: Screenshot
        enabled: screenshotProc.capturing
        function onCaptured(path, ok) {
            if (path !== screenshotProc.screenshotPath)
                return;
            screenshotProc.capturing = false;
            screenshotProc.running = false;
            screenshotProc.exited(ok ? 0 : 1, 0);
        }
    }
}
