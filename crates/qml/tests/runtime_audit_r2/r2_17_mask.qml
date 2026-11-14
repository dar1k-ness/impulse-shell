import QtQuick
Item {
    TextInput { id: a; validator: IntValidator { bottom: 10; top: 99 }
        text: "5" }
    TextInput { id: b; validator: DoubleValidator { bottom: 0; top: 1; decimals: 2 }
        text: "0.555" }
    TextInput { id: c; validator: RegularExpressionValidator { regularExpression: /[a-c]+/ }
        text: "abd" }
    TextInput { id: d; inputMask: "99-99;_"; text: "1" }
    TextInput { id: e; inputMask: ">AAA-000"; text: "ab1" }
    TextInput { id: f; validator: IntValidator { id: iv; bottom: 0; top: 5 }
        text: "7" }
    Component.onCompleted: {
        console.log("a", a.acceptableInput, JSON.stringify(a.text))
        a.text = "50"; console.log("a2", a.acceptableInput)
        a.text = "abc"; console.log("a3", a.acceptableInput, JSON.stringify(a.text))
        console.log("b", b.acceptableInput); b.text = "0.5"; console.log("b2", b.acceptableInput)
        console.log("c", c.acceptableInput); c.text = "abc"; console.log("c2", c.acceptableInput); c.text = ""; console.log("c3", c.acceptableInput)
        console.log("d", JSON.stringify(d.text), JSON.stringify(d.displayText), d.acceptableInput, d.cursorPosition, d.maximumLength)
        d.text = "1234"; console.log("d2", JSON.stringify(d.text), JSON.stringify(d.displayText), d.acceptableInput)
        console.log("e", JSON.stringify(e.text), JSON.stringify(e.displayText), e.acceptableInput)
        console.log("f", f.acceptableInput); iv.top = 9; console.log("f2", f.acceptableInput)
        f.validator = null; console.log("f3", f.acceptableInput)
        d.inputMask = ""; console.log("d3", JSON.stringify(d.text), d.maximumLength)
    }
}
