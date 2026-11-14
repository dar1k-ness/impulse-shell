import QtQuick
import QtQuick.Controls
Item {
    ComboBox { id: a; model: ["x", "y"]; displayText: "Pick: " + currentText }
    ComboBox { id: b; model: ["x", "y"] }
    ComboBox { id: c; model: ["x", "y"]; currentIndex: 1 }
    Component.onCompleted: {
        console.log("a", a.displayText)
        b.displayText = "custom"; console.log("b", b.displayText); b.currentIndex = 1; console.log("b", b.displayText, b.currentText)
        c.model = ["q", "r", "s"]; console.log("c after model change:", c.currentIndex, c.currentText)
        c.currentIndex = 2; c.model = ["only"]; console.log("c shrink:", c.currentIndex, JSON.stringify(c.currentText))
        console.log("currentValue", c.currentValue)
    }
}
