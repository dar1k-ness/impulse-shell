// QQmlInstantiator without a `model` holds one object, also when it starts with
// `active: false` and is switched on later. activeChanged comes first, then per object
// countChanged, objectChanged (for the first one) and objectAdded; switching off removes the
// objects, then objectChanged and countChanged.
// qt: obj1:one add1:0 count1:1 | start 1,one,1,0,,1,0, | active2:true obj2:two add2:0 count2:1 | i2 on 1,two | obj3:three0 add3:0 add3:1 count3:2 | i3 on 2,three0,three1 | active1:false rem1:0 obj1:null count1:0 | i1 off 0, | active1:true obj1:one add1:0 count1:1 | i1 on 1,one | active2:false rem2:0 obj2:null count2:0 | i2 off 0, | rem3:0 rem3:1 obj3:null obj3:three0 add3:0 count3:1 | i3 model 1,three0
import QtQuick
import QtQml
Item {
    id: root
    property string result: ""
    Instantiator {
        id: i1
        QtObject {
            property string t: "one"
        }
        onObjectAdded: (i, o) => root.result += "add1:" + i + " "
        onObjectRemoved: (i, o) => root.result += "rem1:" + i + " "
        onCountChanged: root.result += "count1:" + count + " "
        onObjectChanged: root.result += "obj1:" + (object ? object.t : object) + " "
        onActiveChanged: root.result += "active1:" + active + " "
    }
    Instantiator {
        id: i2
        active: false
        QtObject {
            property string t: "two"
        }
        onObjectAdded: (i, o) => root.result += "add2:" + i + " "
        onObjectRemoved: (i, o) => root.result += "rem2:" + i + " "
        onCountChanged: root.result += "count2:" + count + " "
        onObjectChanged: root.result += "obj2:" + (object ? object.t : object) + " "
        onActiveChanged: root.result += "active2:" + active + " "
    }
    Instantiator {
        id: i3
        active: false
        model: 2
        QtObject {
            property string t: "three" + index
        }
        onObjectAdded: (i, o) => root.result += "add3:" + i + " "
        onObjectRemoved: (i, o) => root.result += "rem3:" + i + " "
        onCountChanged: root.result += "count3:" + count + " "
        onObjectChanged: root.result += "obj3:" + (object ? object.t : object) + " "
    }
    Component.onCompleted: {
        result += "| start " + [i1.count, i1.object ? i1.object.t : i1.object, i1.model, i2.count, i2.object, i2.model, i3.count, i3.object] + " | "
        i2.active = true
        result += "| i2 on " + [i2.count, i2.object ? i2.object.t : i2.object] + " | "
        i3.active = true
        result += "| i3 on " + [i3.count, i3.object ? i3.object.t : i3.object, i3.objectAt(1).t] + " | "
        i1.active = false
        result += "| i1 off " + [i1.count, i1.object] + " | "
        i1.active = true
        result += "| i1 on " + [i1.count, i1.object ? i1.object.t : i1.object] + " | "
        i2.active = false
        i2.active = false
        result += "| i2 off " + [i2.count, i2.object] + " | "
        i3.model = 1
        result += "| i3 model " + [i3.count, i3.object ? i3.object.t : i3.object]
    }
}
