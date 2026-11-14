import QtQuick
Item {
    TextInput { id: a; maximumLength: -1; text: "hello" }
    TextInput { id: b; maximumLength: -5; text: "hello" }
    TextInput { id: c; maximumLength: 0; text: "hello" }
    TextInput { id: e; text: "hello" }
    Component.onCompleted: {
        console.log("a", a.maximumLength, JSON.stringify(a.text))
        a.insert(5, " world"); console.log("a2", JSON.stringify(a.text))
        a.text = "replaced text"; console.log("a3", JSON.stringify(a.text))
        console.log("b", b.maximumLength, JSON.stringify(b.text))
        console.log("c", c.maximumLength, JSON.stringify(c.text))
        c.insert(0, "x"); console.log("c2", JSON.stringify(c.text))
        e.maximumLength = -1; console.log("e", JSON.stringify(e.text)); e.insert(0, "ab"); console.log("e2", JSON.stringify(e.text))
        e.maximumLength = 3; console.log("e3", JSON.stringify(e.text)); e.insert(0, "ab"); console.log("e4", JSON.stringify(e.text))
    }
}
