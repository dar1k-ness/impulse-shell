import QtQuick
import QtQuick.Controls
Item {
    Action { id: act; text: "Act"; enabled: false; checkable: true; checked: true; onTriggered: console.log("triggered") }
    Button { id: b; action: act }
    Button { id: b2; action: act; text: "own"; enabled: true }
    Component.onCompleted: {
        console.log("b", b.text, b.enabled, b.checkable, b.checked)
        console.log("b2", b2.text, b2.enabled)
        act.enabled = true; act.text = "New"
        console.log("b", b.text, b.enabled, "b2", b2.text, b2.enabled)
        b.action = null; console.log("no action", JSON.stringify(b.text), b.enabled, b.checkable, b.checked)
    }
}
