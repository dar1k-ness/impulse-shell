import QtQuick
Item {
    id: root
    width: 200; height: 200
    ListView {
        id: plain
        width: 200; height: 100
        model: 40
        delegate: Rectangle { required property int index; width: 200; height: 20 }
    }
    ListView {
        id: parted
        width: 200; height: 100
        model: 40
        section.property: "modelData"; section.criteria: ViewSection.FirstCharacter
        delegate: Rectangle { required property int index; width: 200; height: 20 }
    }
    function made(v) { var n = 0; for (var i = 0; i < v.count; i++) if (v.itemAtIndex(i)) n++; return n }
    Timer { interval: 200; running: true; onTriggered: {
        console.log("plain rows", root.made(plain), "of", plain.count)
        console.log("parted rows", root.made(parted), "of", parted.count)
        console.log("done")
    } }
}
