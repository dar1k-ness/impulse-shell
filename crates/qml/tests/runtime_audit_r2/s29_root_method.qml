import QtQuick
TextInput {
    id: r
    text: "hello"
    Item { Component.onCompleted: { console.log(typeof selectAll); selectAll(); console.log("sel", r.selectedText) } }
}
