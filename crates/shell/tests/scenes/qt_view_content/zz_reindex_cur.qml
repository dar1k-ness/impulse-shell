// qt: at 2000 50/n50/50,51/n51/51,52/n52/52,53/n53/53,54/n54/54,55/n55/55,56/n56/56,57/n57/57,58/n58/58,59/n59/59,60/n60/60,61/n61/61,62/n62/62,63/n63/63,64/n64/64,65/n65/65,66/n66/66,67/n67/67,68/n68/68 | row put first 51/n50/51,52/n51/52,53/n52/53,54/n53/54,55/n54/55,56/n55/56,57/n56/57,58/n57/58,59/n58/59,60/n59/60,61/n60/61,62/n61/62,63/n62/63,64/n63/64,65/n64/65,66/n65/66,67/n66/67,68/n67/68,69/n68/69 | three taken 48/n50/48,49/n51/49,50/n52/50,51/n53/51,52/n54/52,53/n55/53,54/n56/54,55/n57/55,56/n58/56,57/n59/57,58/n60/58,59/n61/59,60/n62/60,61/n63/61,62/n64/62,63/n65/63,64/n66/64,65/n67/65,66/n68/66
import QtQuick
Item {
    id: root
    width: 300; height: 400
    property string result: ""
    property int k: 0
    property var out: []
    ListModel { id: lm; Component.onCompleted: { for (let i = 0; i < 100; i++) append({ name: "n" + i }) } }
    function rows() {
        const l = []
        for (const c of v.contentItem.children)
            if (c.isRow && c.visible) l.push(c)
        l.sort((a, b) => a.y - b.y)
        return l.map(c => c.seen).join(",")
    }
    ListView {
        id: v
        width: 300; height: 400
        model: lm
        currentIndex: 55
        delegate: Item {
            width: 300; height: 40
            readonly property bool isRow: true
            property string seen: index + "/" + name + "/" + model.index
        }
    }
    Timer {
        interval: 60; repeat: true; running: true
        onTriggered: {
            root.k++
            if (root.k == 1) v.contentY = 2000
            else if (root.k == 2) { root.out.push("at 2000 " + root.rows()); lm.insert(0, { name: "new" }) }
            else if (root.k == 3) { root.out.push("row put first " + root.rows()); lm.remove(0, 3) }
            else { root.out.push("three taken " + root.rows()); running = false; root.result = root.out.join(" | ") }
        }
    }
}
