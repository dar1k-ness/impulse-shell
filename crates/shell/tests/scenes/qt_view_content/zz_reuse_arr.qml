// qt: start rows 0:0/t0/0/t0,1:1/t1/1/t1,2:2/t2/2/t2,3:3/t3/3/t3,4:4/t4/4/t4,5:5/t5/5/t5,6:6/t6/6/t6,7:7/t7/7/t7,8:8/t8/8/t8,9:9/t9/9/t9,10:10/t10/10/t10,11:11/t11/11/t11,12:12/t12/12/t12,13:13/t13/13/t13,14:14/t14/14/t14,15:15/t15/15/t15,16:16/t16/16/t16,17:17/t17/17/t17,18:18/t18/18/t18 reused 0 | 2000 rows 0:0/t0/0/t0,50:50/t50/50/t50,51:51/t51/51/t51,52:52/t52/52/t52,53:53/t53/53/t53,54:54/t54/54/t54,55:55/t55/55/t55,56:56/t56/56/t56,57:57/t57/57/t57,58:58/t58/58/t58,59:59/t59/59/t59,60:60/t60/60/t60,61:61/t61/61/t61,62:62/t62/62/t62,63:63/t63/63/t63,64:64/t64/64/t64,65:65/t65/65/t65,66:66/t66/66/t66,67:67/t67/67/t67,68:68/t68/68/t68 reused 18 | 0 rows 0:0/t0/0/t0,1:1/t1/1/t1,2:2/t2/2/t2,3:3/t3/3/t3,4:4/t4/4/t4,5:5/t5/5/t5,6:6/t6/6/t6,7:7/t7/7/t7,8:8/t8/8/t8,9:9/t9/9/t9,10:10/t10/10/t10,11:11/t11/11/t11,12:12/t12/12/t12,13:13/t13/13/t13,14:14/t14/14/t14,15:15/t15/15/t15,16:16/t16/16/t16,17:17/t17/17/t17,18:18/t18/18/t18,68:68/t68/68/t68 reused 36
import QtQuick
Item {
    id: root
    width: 300; height: 400
    property string result: ""
    property var steps: [2000, 0]
    property int k: -1
    property var out: []
    property var arr: { const a = []; for (let i = 0; i < 100; i++) a.push({ t: "t" + i }); return a }
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
        model: root.arr; reuseItems: true; property int reused: 0
        delegate: Item {
            width: 300; height: 40; ListView.onReused: v.reused++
            readonly property bool isRow: true
            property string seen: index + "/" + modelData.t + "/" + model.index + "/" + model.modelData.t
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
