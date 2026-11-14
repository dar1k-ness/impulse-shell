pragma Singleton
import QtQuick
import Quickshell
Singleton {
    property string name: "single"
    property QtObject inner: QtObject { Component.onDestruction: console.log("destroyed singleton's object") }
    Component.onDestruction: console.log("destroyed singleton")
}
