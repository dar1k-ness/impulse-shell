// A property declared in QML under the name of a C++ property of the type hides that property
// (QQmlPropertyCache::append): it reads, binds and notifies as any declared property and takes
// what is written to it, and nothing the C++ property does happens. `children`, `states`,
// `transitions`, `resources` and `data` of an Item as a list and as a scalar, `running` and
// `interval` of a Timer, `text` of a Text. Each entry: the value and a binding on it, both again
// after a write, then the change signals of that write.
// qt: cl [1,{"k":2}] [1,{"k":2}] 2/1 [7] [7] 2/1 c; cs 5 6 6 7 1 c; sl a,b a+b z z [] s; ss one one! two two! s; tl 1,2 2 9,8,7 3 t; ts 1.5 3 2.5 5 t; rl 1,2 2 5 1 r; rs res res! x x! r; dl 1,2 2 5 1 0 d; ds 4 5 8 9 d; tm no no? yes yes? RI; tx 3 4 0 8 9 0 T
import QtQuick
Item {
    id: root
    property string result: ""
    property string log: ""
    Item {
        id: cl
        property list<var> children: [1, { "k": 2 }]
        property string seen: JSON.stringify(cl.children)
        onChildrenChanged: root.log += "c"
        Item {}
        QtObject {}
    }
    Item {
        id: cs
        property int children: 5
        property int seen: cs.children + 1
        onChildrenChanged: root.log += "c"
        Item {}
    }
    Item {
        id: sl
        property list<string> states: ["a", "b"]
        property string seen: sl.states.join("+")
        onStatesChanged: root.log += "s"
    }
    Item {
        id: ss
        property string states: "one"
        property string seen: ss.states + "!"
        onStatesChanged: root.log += "s"
    }
    Item {
        id: tl
        property list<int> transitions: [1, 2]
        property int seen: tl.transitions.length
        onTransitionsChanged: root.log += "t"
    }
    Item {
        id: ts
        property real transitions: 1.5
        property real seen: ts.transitions * 2
        onTransitionsChanged: root.log += "t"
    }
    Item {
        id: rl
        property list<var> resources: [1, 2]
        property int seen: rl.resources.length
        onResourcesChanged: root.log += "r"
        QtObject {}
    }
    Item {
        id: rs
        property string resources: "res"
        property string seen: rs.resources + "!"
        onResourcesChanged: root.log += "r"
        QtObject {}
    }
    Item {
        id: dl
        property list<var> data: [1, 2]
        property int seen: dl.data.length
        onDataChanged: root.log += "d"
    }
    Item {
        id: ds
        property int data: 4
        property int seen: ds.data + 1
        onDataChanged: root.log += "d"
    }
    Timer {
        id: tm
        property string running: "no"
        property string interval: tm.running + "?"
        onRunningChanged: root.log += "R"
        onIntervalChanged: root.log += "I"
    }
    Text {
        id: tx
        property int text: 3
        property int seen: tx.text + 1
        onTextChanged: root.log += "T"
    }
    function take() { var l = log; log = ""; return l }
    Component.onCompleted: {
        // The signals of the creation itself are not this case.
        take()
        result += "cl " + JSON.stringify(cl.children) + " " + cl.seen + " " + cl.data.length + "/" + cl.resources.length
        cl.children = [7]
        result += " " + JSON.stringify(cl.children) + " " + cl.seen + " " + cl.data.length + "/" + cl.resources.length + " " + take() + "; "
        result += "cs " + cs.children + " " + cs.seen
        cs.children = 6
        result += " " + cs.children + " " + cs.seen + " " + cs.data.length + " " + take() + "; "
        result += "sl " + sl.states + " " + sl.seen
        sl.states = ["z"]
        result += " " + sl.states + " " + sl.seen + " [" + sl.state + "] " + take() + "; "
        result += "ss " + ss.states + " " + ss.seen
        ss.states = "two"
        result += " " + ss.states + " " + ss.seen + " " + take() + "; "
        result += "tl " + tl.transitions + " " + tl.seen
        tl.transitions = [9, 8, 7]
        result += " " + tl.transitions + " " + tl.seen + " " + take() + "; "
        result += "ts " + ts.transitions + " " + ts.seen
        ts.transitions = 2.5
        result += " " + ts.transitions + " " + ts.seen + " " + take() + "; "
        result += "rl " + rl.resources + " " + rl.seen
        rl.resources = [5]
        result += " " + rl.resources + " " + rl.seen + " " + take() + "; "
        result += "rs " + rs.resources + " " + rs.seen
        rs.resources = "x"
        result += " " + rs.resources + " " + rs.seen + " " + take() + "; "
        result += "dl " + dl.data + " " + dl.seen
        dl.data = [5]
        result += " " + dl.data + " " + dl.seen + " " + dl.children.length + " " + take() + "; "
        result += "ds " + ds.data + " " + ds.seen
        ds.data = 8
        result += " " + ds.data + " " + ds.seen + " " + take() + "; "
        result += "tm " + tm.running + " " + tm.interval
        tm.running = "yes"
        result += " " + tm.running + " " + tm.interval + " " + take() + "; "
        result += "tx " + tx.text + " " + tx.seen + " " + tx.implicitWidth
        tx.text = 8
        result += " " + tx.text + " " + tx.seen + " " + tx.implicitWidth + " " + take()
    }
}
