// qt: start rows 0:0/n0/n0/0/0/0,1:1/n1/n1/1/1/1,2:2/n2/n2/2/2/2,3:3/n3/n3/3/3/3,4:4/n4/n4/4/4/4,5:5/n5/n5/5/5/5,6:6/n6/n6/6/6/6,7:7/n7/n7/7/7/7,8:8/n8/n8/8/8/8,9:9/n9/n9/9/9/9,10:10/n10/n10/10/10/10,11:11/n11/n11/11/11/11,12:12/n12/n12/12/12/12,13:13/n13/n13/13/13/13,14:14/n14/n14/14/14/14,15:15/n15/n15/15/15/15,16:16/n16/n16/16/16/16,17:17/n17/n17/17/17/17,18:18/n18/n18/18/18/18 reused 0 | 2000 rows 0:0/n0/n0/0/0/0,50:50/n1/n50/50/50/50,51:51/n2/n51/51/51/51,52:52/n3/n52/52/52/52,53:53/n4/n53/53/53/53,54:54/n5/n54/54/54/54,55:55/n6/n55/55/55/55,56:56/n7/n56/56/56/56,57:57/n8/n57/57/57/57,58:58/n9/n58/58/58/58,59:59/n10/n59/59/59/59,60:60/n11/n60/60/60/60,61:61/n12/n61/61/61/61,62:62/n13/n62/62/62/62,63:63/n14/n63/63/63/63,64:64/n15/n64/64/64/64,65:65/n16/n65/65/65/65,66:66/n17/n66/66/66/66,67:67/n18/n67/67/67/67,68:68/n68/n68/68/68/68 reused 18 | 0 rows 0:0/n0/n0/0/0/0,1:1/n1/n1/1/1/1,2:2/n2/n2/2/2/2,3:3/n3/n3/3/3/3,4:4/n4/n4/4/4/4,5:5/n5/n5/5/5/5,6:6/n6/n6/6/6/6,7:7/n7/n7/7/7/7,8:8/n8/n8/8/8/8,9:9/n9/n9/9/9/9,10:10/n10/n10/10/10/10,11:11/n11/n11/11/11/11,12:12/n12/n12/12/12/12,13:13/n13/n13/13/13/13,14:14/n14/n14/14/14/14,15:15/n15/n15/15/15/15,16:16/n16/n16/16/16/16,17:17/n17/n17/17/17/17,18:18/n18/n18/18/18/18,68:68/n68/n68/68/68/68 reused 36
import QtQuick
Item {
    id: root
    width: 300; height: 400
    property string result: ""
    property var steps: [2000, 0]
    property int k: -1
    property var out: []
    property list<QtObject> objs; Component { id: oc; QtObject { property string name; property int n } } Component.onCompleted: { const a = []; for (let i = 0; i < 100; i++) a.push(oc.createObject(root, { name: "n" + i, n: i })); objs = a }
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
        model: root.objs; reuseItems: true; property int reused: 0
        delegate: Item {
            width: 300; height: 40; ListView.onReused: v.reused++
            readonly property bool isRow: true
            property string seen: index + "/" + name + "/" + model.name + "/" + model.index + "/" + modelData.n + "/" + model.modelData.n
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
