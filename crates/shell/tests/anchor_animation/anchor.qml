import QtQuick
Item {
    id: root
    property string log: ""
    function say(...a) { const s = a.join(" "); log += s + "\n"; console.log(s) }
    width: 400; height: 300
    property real lin: 0
    property bool on: false
    property bool back: false
    // a: both axes, AnchorAnimation with easing
    Rectangle { id: a; width: 40; height: 20; anchors.left: parent.left; anchors.top: parent.top
        states: State { name: "on"; when: root.on; AnchorChanges { target: a; anchors.left: undefined; anchors.right: root.right; anchors.top: undefined; anchors.bottom: root.bottom } }
        transitions: Transition { AnchorAnimation { duration: 160; easing.type: Easing.OutCubic } } }
    // b: width changes (left stays, right added), default duration 250
    Rectangle { id: b; y: 50; width: 40; height: 20; anchors.left: parent.left; anchors.leftMargin: 10
        states: State { name: "on"; when: root.on; AnchorChanges { target: b; anchors.right: root.right } }
        transitions: Transition { AnchorAnimation {} } }
    // c: transition without AnchorAnimation: when does it move?
    Rectangle { id: c; y: 100; width: 40; height: 20; anchors.left: parent.left; property real p: 0
        states: State { name: "on"; when: root.on; AnchorChanges { target: c; anchors.left: undefined; anchors.right: root.right }
            PropertyChanges { target: c; p: 10 } }
        transitions: Transition { NumberAnimation { property: "p"; duration: 96 } } }
    // d, e: one state changes both, AnchorAnimation only targets d; f follows d by anchor
    Rectangle { id: d; y: 150; width: 40; height: 20; anchors.left: parent.left }
    Rectangle { id: e; y: 200; width: 40; height: 20; anchors.left: parent.left }
    Rectangle { id: f; y: 150; width: 10; height: 10; anchors.left: d.right }
    Item { id: host
        states: State { name: "on"; when: root.on
            AnchorChanges { target: d; anchors.left: undefined; anchors.horizontalCenter: root.horizontalCenter }
            AnchorChanges { target: e; anchors.left: undefined; anchors.horizontalCenter: root.horizontalCenter } }
        transitions: Transition { AnchorAnimation { targets: d; duration: 160 } } }
    // g: reversed half way
    Rectangle { id: g; y: 250; width: 40; height: 20; anchors.left: parent.left
        states: State { name: "r"; AnchorChanges { target: g; anchors.left: undefined; anchors.right: root.right } }
        transitions: Transition { id: gt; AnchorAnimation { duration: 160 }
            onRunningChanged: say("gt runningChanged", g.x.toFixed(2)) } }
    NumberAnimation { id: ref; target: root; property: "lin"; from: 0; to: 1; duration: 320 }
    function sample() {
        say("s", lin.toFixed(3), a.x.toFixed(2), a.y.toFixed(2), b.x.toFixed(2), b.width.toFixed(2), c.x.toFixed(2), c.p.toFixed(2), d.x.toFixed(2), e.x.toFixed(2), f.x.toFixed(2), g.x.toFixed(2))
    }
    onLinChanged: {
        if (lin >= 0.25 && !back) { back = true; say("back"); g.state = ""; say("after back", g.x.toFixed(2)) }
    }
    Component.onCompleted: { on = true; g.state = "r"; ref.start(); say("after on") }
}
