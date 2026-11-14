// qt: start rows 0:0/0/0,1:1/1/2,2:2/2/4,3:3/3/6,4:4/4/8,5:5/5/10,6:6/6/12,7:7/7/14,8:8/8/16,9:9/9/18,10:10/10/20,11:11/11/22,12:12/12/24,13:13/13/26,14:14/14/28,15:15/15/30,16:16/16/32,17:17/17/34,18:18/18/36 reused 0 | 2000 rows 0:0/0/0,50:50/50/100,51:51/51/102,52:52/52/104,53:53/53/106,54:54/54/108,55:55/55/110,56:56/56/112,57:57/57/114,58:58/58/116,59:59/59/118,60:60/60/120,61:61/61/122,62:62/62/124,63:63/63/126,64:64/64/128,65:65/65/130,66:66/66/132,67:67/67/134,68:68/68/136 reused 18 | 0 rows 0:0/0/0,1:1/1/2,2:2/2/4,3:3/3/6,4:4/4/8,5:5/5/10,6:6/6/12,7:7/7/14,8:8/8/16,9:9/9/18,10:10/10/20,11:11/11/22,12:12/12/24,13:13/13/26,14:14/14/28,15:15/15/30,16:16/16/32,17:17/17/34,18:18/18/36,68:68/68/136 reused 36
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
            required property int index; required property var modelData; property string seen: index + "/" + modelData + "/" + (index * 2)
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
