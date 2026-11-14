import QtQuick
Item {
    objectName: "outer"
    Item {
        id: comp
        objectName: "docroot"
        property QtObject q: QtObject { Component.onCompleted: console.log("unqualified parent in QtObject:", parent.objectName) }
    }
}
