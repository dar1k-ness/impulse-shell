import QtQuick
Item {
    id: root
    QtObject { id: obj; property string name: "nm" }
    Repeater { id: r1; model: ({ a: 1, b: "x" }); delegate: Item { Component.onCompleted: console.log("r1 delegate", index, typeof modelData, JSON.stringify(modelData)) } }
    Repeater { id: r2; model: true; delegate: Item { Component.onCompleted: console.log("r2 delegate", index, modelData) } }
    Repeater { id: r3; model: null; delegate: Item { Component.onCompleted: console.log("r3 delegate", index) } }
    Repeater { id: r4; model: [obj, obj]; delegate: Item { Component.onCompleted: console.log("r4 delegate", index, modelData.name) } }
    Repeater { id: r5; model: obj; delegate: Item { Component.onCompleted: console.log("r5 delegate", index, modelData === obj) } }
    Component.onCompleted: { console.log("counts", r1.count, r2.count, r3.count, r4.count, r5.count); console.log("END"); Qt.quit() }
}
