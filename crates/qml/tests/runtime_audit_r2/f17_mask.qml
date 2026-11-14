import QtQuick
Item {
    TextInput { id: d; inputMask: "99-99;_"; text: "1" }
    TextInput { id: g; text: "1234" }
    TextInput { id: h; text: "abcdef" }
    TextInput { id: k; text: "12"; inputMask: "99-99;_"
        onTextChanged: console.log("  k.text", JSON.stringify(text))
        onDisplayTextChanged: console.log("  k.displayText", JSON.stringify(displayText))
        onCursorPositionChanged: console.log("  k.cursor", cursorPosition)
        onMaximumLengthChanged: console.log("  k.max", k.maximumLength)
        onInputMaskChanged: console.log("  k.mask", JSON.stringify(k.inputMask))
        onAcceptableInputChanged: console.log("  k.acc", acceptableInput)
    }
    function st(t) { return JSON.stringify(t.text) + " " + JSON.stringify(t.displayText) + " c" + t.cursorPosition + " s" + t.selectionStart + "-" + t.selectionEnd + " max" + t.maximumLength + " acc" + t.acceptableInput + " mask" + JSON.stringify(t.inputMask) }
    Component.onCompleted: {
        console.log("d0", st(d))
        d.text = "1234"; console.log("d1", st(d))
        d.cursorPosition = 3; d.text = "56"; console.log("d2", st(d))
        d.inputMask = ""; console.log("d3", st(d))
        d.text = "xyz"; console.log("d4", st(d))
        g.inputMask = "99-99;_"; console.log("g1", st(g))
        g.inputMask = "9999"; console.log("g2", st(g))
        g.inputMask = ";"; console.log("g3", st(g))
        g.text = "77"; g.inputMask = ""; console.log("g4", st(g))
        h.maximumLength = 3; h.inputMask = ""; console.log("h1", st(h))
        h.inputMask = "AAAA"; console.log("h2", st(h))
        h.maximumLength = 2; console.log("h3", st(h))
        h.inputMask = ""; console.log("h4", st(h))
        console.log("k0", st(k))
        k.text = "34"; console.log("k1", st(k))
        k.inputMask = ""; console.log("k2", st(k))
        k.text = "9"; k.inputMask = "99-99;_"; console.log("k3", st(k))
    }
}
