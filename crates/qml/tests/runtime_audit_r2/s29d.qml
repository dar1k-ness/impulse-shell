import QtQuick
ListView {
    id: lv
    model: 3
    delegate: Item { Component.onCompleted: if (index == 0) console.log("delegate sees:", typeof positionViewAtIndex, typeof forceActiveFocus, typeof destroy, typeof toString) }
}
