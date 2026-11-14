// qt: start rows 0:0/0/0/0,1:1/1/1/1,2:2/2/2/2,3:3/3/3/3,4:4/4/4/4,5:5/5/5/5,6:6/6/6/6,7:7/7/7/7,8:8/8/8/8,9:9/9/9/9,10:10/10/10/10,11:11/11/11/11,12:12/12/12/12,13:13/13/13/13,14:14/14/14/14,15:15/15/15/15,16:16/16/16/16,17:17/17/17/17,18:18/18/18/18 reused 0 | 2000 rows 0:0/0/0/0,50:50/50/50/50,51:51/51/51/51,52:52/52/52/52,53:53/53/53/53,54:54/54/54/54,55:55/55/55/55,56:56/56/56/56,57:57/57/57/57,58:58/58/58/58,59:59/59/59/59,60:60/60/60/60,61:61/61/61/61,62:62/62/62/62,63:63/63/63/63,64:64/64/64/64,65:65/65/65/65,66:66/66/66/66,67:67/67/67/67,68:68/68/68/68 reused 18 | 0 rows 0:0/0/0/0,1:1/1/1/1,2:2/2/2/2,3:3/3/3/3,4:4/4/4/4,5:5/5/5/5,6:6/6/6/6,7:7/7/7/7,8:8/8/8/8,9:9/9/9/9,10:10/10/10/10,11:11/11/11/11,12:12/12/12/12,13:13/13/13/13,14:14/14/14/14,15:15/15/15/15,16:16/16/16/16,17:17/17/17/17,18:18/18/18/18,68:68/68/68/68 reused 36
import QtQuick
Item {
    id: root
    width: 300; height: 400
    property string result: ""
    property var steps: [2000, 0]
    property int k: -1
    property var out: []
    
    function rows() {
        const l = []
        for (const c of v.contentItem.children)
            if (c.isRow && c.visible) l.push(c)
        l.sort((a, b) => a.y - b.y)
        return l.map(c => Math.round(c.y / 40) + ":" + c.seen).join(",")
    }
    ListView {
        id: v
        width: 300; height: 400
        model: 100; reuseItems: true; property int reused: 0
        delegate: Item {
            width: 300; height: 40; ListView.onReused: v.reused++
            readonly property bool isRow: true
            property string seen: index + "/" + modelData + "/" + model.index + "/" + model.modelData
        }
    }
    Timer {
        interval: 60; repeat: true; running: true
        onTriggered: {
            root.out.push((root.k < 0 ? "start" : root.steps[root.k]) + " rows " + root.rows() + " reused " + v.reused)
            root.k++
            if (root.k < root.steps.length) {
                v.contentY = root.steps[root.k]
            } else {
                running = false
                root.result = root.out.join(" | ")
            }
        }
    }
}
