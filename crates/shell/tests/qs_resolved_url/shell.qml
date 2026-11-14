import QtQuick
import Rustshell
ShellRoot {
    Probe { id: probe }
    Timer { interval: 1500; running: true; onTriggered: { var l = probe.report(); for (var k = 0; k < l.length; k++) console.log("CASE", l[k]); Qt.quit() } }
}
