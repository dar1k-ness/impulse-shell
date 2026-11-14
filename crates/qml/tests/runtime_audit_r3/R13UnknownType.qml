import QtQuick
Item {
    Nope { id: x }
    Component.onCompleted: console.log("created", x)
}
