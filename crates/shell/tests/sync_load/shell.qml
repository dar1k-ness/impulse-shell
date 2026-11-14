import QtQuick
import Quickshell
ShellRoot {
    QsProbe {
        onFinished: {
            for (var l of report.split("\n"))
                console.log("CASE", l)
            Qt.quit()
        }
    }
}
