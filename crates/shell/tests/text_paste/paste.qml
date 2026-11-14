// Runs PasteSteps.qml in Qt: `h` copies each step's text to the clipboard.
import QtQuick
PasteSteps {
    id: s
    TextInput { id: h }
    Component.onCompleted: {
        s.all(() => { if (s.clip !== "") { h.text = s.clip; h.selectAll(); h.copy() } })
        console.log("LOG<" + s.log + ">LOG"); Qt.quit()
    }
}
