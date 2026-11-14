import QtQuick
import QtQuick.Controls
Item {
    TextInput { id: a; text: "abc"
        onTextChanged: console.log("  a.text", JSON.stringify(text))
        onDisplayTextChanged: console.log("  a.displayText", JSON.stringify(displayText))
        onCursorPositionChanged: console.log("  a.cursor", cursorPosition)
        onEchoModeChanged: console.log("  a.echo", a.echoMode)
        onPasswordCharacterChanged: console.log("  a.pwc", passwordCharacter)
        onMaximumLengthChanged: console.log("  a.max", a.maximumLength)
    }
    TextInput { id: b }
    TextInput { id: c; text: "secret"; echoMode: TextInput.Password; passwordCharacter: "*" }
    TextInput { id: e; text: "hid"; echoMode: TextInput.NoEcho }
    TextInput { id: f; text: "edit"; echoMode: TextInput.PasswordEchoOnEdit; passwordCharacter: "#" }
    TextField { id: t; text: "field"
        onTextChanged: console.log("  t.text", JSON.stringify(text))
        onDisplayTextChanged: console.log("  t.displayText", JSON.stringify(displayText))
    }
    property string bound: a.displayText + "|" + t.displayText
    function st(x) { return JSON.stringify(x.text) + " " + JSON.stringify(x.displayText) + " c" + x.cursorPosition + " " + bound }
    Component.onCompleted: {
        console.log("a0", st(a)); console.log("b0", st(b)); console.log("c0", st(c)); console.log("e0", st(e)); console.log("f0", st(f)); console.log("t0", st(t))
        a.text = "abcd"; console.log("a1", st(a))
        a.text = "abcd"; console.log("a2", st(a))
        a.text = ""; console.log("a3", st(a))
        a.text = "xy"; console.log("a4", st(a))
        a.echoMode = TextInput.Password; console.log("a5", st(a).length, a.displayText.length, a.displayText === a.passwordCharacter + a.passwordCharacter)
        a.passwordCharacter = "+"; console.log("a6", st(a))
        a.text = "xyz"; console.log("a7", st(a))
        a.echoMode = TextInput.NoEcho; console.log("a8", st(a))
        a.text = "q"; console.log("a9", st(a))
        a.echoMode = TextInput.Normal; console.log("a10", st(a))
        a.maximumLength = 0; console.log("a11", st(a))
        a.maximumLength = 5; a.text = "1234567"; console.log("a12", st(a))
        a.insert(1, "Z"); console.log("a13", st(a))
        a.remove(0, 2); console.log("a14", st(a))
        a.clear(); console.log("a15", st(a))
        b.text = "new"; console.log("b1", st(b))
        t.text = "f2"; console.log("t1", st(t))
        t.echoMode = TextInput.Password; t.passwordCharacter = "x"; console.log("t2", st(t))
        t.clear(); console.log("t3", st(t))
    }
}
