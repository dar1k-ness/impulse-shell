import QtQuick
import Quickshell
Scope {
    QtObject { Component.onDestruction: console.log("destroyed child") }
    Component.onDestruction: console.log("destroyed root")
    Component.onCompleted: console.log("ready")
}
