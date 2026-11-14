import QtQuick

QtObject {
    property int needed: 1
    Component.onCompleted: console.log("part ready")
    Component.onDestruction: console.log("destroyed part")
}
