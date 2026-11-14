import QtQuick
import QtQuick.Controls
Popup {
    id: p
    visible: true
    Item { Component.onCompleted: { console.log("close:", typeof close, "open:", typeof open, "faf:", typeof forceActiveFocus); close(); console.log("visible", p.visible) } }
}
