import QtQuick
Item {
    TextInput { id: a; maximumLength: -1; text: "hello" }
    TextInput { id: b; maximumLength: 3; text: "hello" }
    TextInput { id: c; text: "hello"; }
    Component.onCompleted: {
        console.log("a", a.maximumLength, JSON.stringify(a.text))
        console.log("b", b.maximumLength, JSON.stringify(b.text))
        c.maximumLength = 2; console.log("c", JSON.stringify(c.text), c.cursorPosition)
        c.text = "abcdef"; console.log("c2", JSON.stringify(c.text), c.cursorPosition)
        c.maximumLength = 32767; console.log("c3", c.maximumLength)
        c.maximumLength = 40000; console.log("c4", c.maximumLength)
    }
}
