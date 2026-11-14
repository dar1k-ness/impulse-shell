// qt: set 1;direct 204;early 101;view 1 101;mid 101;set 50;direct 304;early 150;mid 150;moved;set 50;direct 404;early 250;mid 250;late view 50 250;set 1;direct 504;early 201;view 50 250;mid 201;set 51;direct 804;early 351;mid 351;late view 51 351;later 351;set 2;direct 1104;early 302;view 51 351;mid 302;later 302;back;set 1;direct 1304;early 401;view 1 401;mid 401;later 401;set 2;direct 1404;early 402;view 2 402;mid 402;later 402;set 50;direct 1604;early 450;mid 450;late view 1 401;later 450;set 51;direct 1704;early 451;mid 451;late view 2 402;later 451
// A ListView's delegate taken from the pool for another row of a list of objects: its model
// item stays connected to the object it had when one of its properties was first read
// through it (QQmlDMObjectDataMetaObject::createProperty connects once), so a write to the
// new row's object does not reach the readers of `model.n`, and a write to that first
// object makes them read the new row's.
import QtQuick
import QtQml.Models
Item {
    id: root
    width: 300; height: 400
    property string result: ""
    property var out: []
    property bool live: false
    property bool late: false
    property int step: 0
    property list<QtObject> objs
    property list<QtObject> some
    Component { id: oc; QtObject { property string name; property int n } }
    Component.onCompleted: {
        const a = []
        for (let i = 0; i < 100; i++)
            a.push(oc.createObject(root, { name: "n" + i, n: i }))
        objs = a
        some = [a[1], a[2], a[50], a[51]]
    }
    function say(s) {
        if (live)
            out.push(s)
    }
    function set(i, n) {
        out.push("set " + i)
        live = true
        objs[i].n = n
        live = false
    }
    property int direct: some.length == 4 ? some[0].n + some[1].n + some[2].n + some[3].n : 0
    onDirectChanged: say("direct " + direct)
    ListView {
        id: v
        width: 300; height: 400
        reuseItems: true
        delegate: Item {
            width: 300; height: 40
            required property var model
            property int b: model.n
            onBChanged: root.say("view " + model.index + " " + b)
        }
    }
    Instantiator {
        model: root.some
        delegate: QtObject {
            required property var model
            property int b: model.n
            onBChanged: root.say("early " + b)
        }
    }
    ListView {
        id: w
        width: 300; height: 400
        reuseItems: true
        delegate: Item {
            width: 300; height: 40
            required property var model
            property int b: root.late ? model.n : -1
            onBChanged: root.say("late view " + model.index + " " + b)
        }
    }
    Instantiator {
        id: mid
        active: false
        model: root.some
        delegate: QtObject {
            required property var model
            property int b: model.n
            onBChanged: root.say("mid " + b)
        }
    }
    Instantiator {
        id: later
        active: false
        model: root.some
        delegate: QtObject {
            required property var model
            property int b: model.n
            onBChanged: root.say("later " + b)
        }
    }
    Timer {
        interval: 60; repeat: true; running: true
        onTriggered: {
            switch (root.step++) {
            case 0:
                v.model = root.objs
                w.model = root.objs
                break
            case 1:
                mid.active = true
                root.set(1, 101)
                root.set(50, 150)
                break
            case 2:
                v.contentY = 2000
                w.contentY = 2000
                break
            case 3:
                root.out.push("moved")
                root.late = true
                root.set(50, 250)
                root.set(1, 201)
                later.active = true
                break
            case 4:
                root.set(51, 351)
                root.set(2, 302)
                v.contentY = 0
                w.contentY = 0
                break
            case 5:
                root.out.push("back")
                root.set(1, 401)
                root.set(2, 402)
                root.set(50, 450)
                root.set(51, 451)
                running = false
                root.result = root.out.join(";")
            }
        }
    }
}
