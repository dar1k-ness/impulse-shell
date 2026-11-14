import QtQuick
import QtQuick.Controls
Item {
    ComboBox { id: a; model: ["x", "y"]; displayText: "Pick: " + currentText }
    ComboBox { id: b; model: ["x", "y"]
        onDisplayTextChanged: console.log("b displayTextChanged", displayText)
        onCurrentValueChanged: console.log("b currentValueChanged", currentValue)
    }
    ComboBox { id: c; model: ["x", "y"]; currentIndex: 1
        onCurrentIndexChanged: console.log("c currentIndexChanged", currentIndex)
    }
    ComboBox { id: d; model: [{ t: "One", v: 1 }, { t: "Two", v: 2 }, { t: "Three", v: 3 }]
        textRole: "t"; valueRole: "v"; currentValue: 2 }
    ComboBox { id: e; model: ["p", "q"]; currentIndex: -1 }
    ComboBox { id: f }
    Component.onCompleted: {
        console.log("a", a.displayText, a.currentValue)
        console.log("b0", b.displayText, b.currentValue)
        b.displayText = "custom"; console.log("b", b.displayText)
        b.currentIndex = 1; console.log("b", b.displayText, b.currentText, b.currentValue)
        b.displayText = undefined; console.log("b reset", b.displayText)
        b.currentIndex = 0; console.log("b", b.displayText)
        c.model = ["q", "r", "s"]; console.log("c after model change:", c.currentIndex, c.currentText)
        c.currentIndex = 2; c.model = ["only"]; console.log("c shrink:", c.currentIndex, JSON.stringify(c.currentText))
        console.log("currentValue", c.currentValue)
        c.model = []; console.log("c empty:", c.currentIndex, JSON.stringify(c.currentText), c.currentValue)
        console.log("d", d.currentIndex, d.currentText, d.currentValue)
        d.currentValue = 3; console.log("d", d.currentIndex, d.currentText, d.currentValue)
        d.model = [{ t: "Tri", v: 3 }, { t: "Uno", v: 1 }]; console.log("d model", d.currentIndex, d.currentText, d.currentValue)
        d.currentIndex = 1; console.log("d idx", d.currentIndex, d.currentText, d.currentValue)
        d.model = [{ t: "A", v: 7 }, { t: "B", v: 8 }]; console.log("d model2", d.currentIndex, d.currentText, d.currentValue)
        d.currentValue = 99; console.log("d missing", d.currentIndex, JSON.stringify(d.currentText), d.currentValue)
        console.log("e", e.currentIndex, JSON.stringify(e.currentText), e.currentValue)
        e.model = ["z"]; console.log("e model", e.currentIndex, e.currentText)
        console.log("f", f.currentIndex, f.count, f.currentValue)
        f.model = ["m", "n"]; console.log("f model", f.currentIndex, f.currentText, f.currentValue)
    }
}
