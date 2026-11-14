import QtQuick
import QtQuick.Controls
Item {
    id: top
    function grabFocus() { return "top.grabFocus" }
    component TI: TextInput {
        id: ti
        text: "hello"
        function open() { return "ti.open" }
        Item {
            id: inner
            Component.onCompleted: {
                console.log("A", typeof selectAll, typeof deselect, typeof open, typeof grabFocus, typeof nothing)
                selectAll(); console.log("A sel", ti.selectedText)
                console.log("A open", open())
                forceActiveFocus(); console.log("A focus inner:", inner.activeFocus, "ti:", ti.activeFocus)
            }
            QtObject { Component.onCompleted: console.log("B", typeof selectAll, typeof forceActiveFocus) }
        }
    }
    component LV: ListView {
        id: lv
        model: 3
        delegate: Item { Component.onCompleted: if (index == 0) console.log("C", typeof positionViewAtIndex, typeof forceActiveFocus, typeof destroy, typeof toString) }
    }
    component PP: Popup {
        id: p
        visible: true
        Item { Component.onCompleted: { console.log("D", typeof close, typeof open); close(); console.log("D visible", p.visible) } }
    }
    TI {} LV {} PP {}
    Item { Component.onCompleted: console.log("E", typeof close, typeof selectAll, typeof positionViewAtIndex, typeof grabFocus) }
}
