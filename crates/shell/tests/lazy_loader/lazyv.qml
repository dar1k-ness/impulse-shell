import QtQuick
import Quickshell
Scope {
    id: root
    property var m: ["a"]
    property var n: ["x"]
    Variants { id: v1; model: root.m
        Scope { id: inst; required property var modelData
            property var la: xa
            property var lb: xb
            Component.onCompleted: console.log("v1 inst completed", modelData, xa.active, xb.loading)
            LazyLoader { id: xa; active: true; onActiveChanged: console.log("v1", inst.modelData, "xa active", active)
                QtObject { Component.onCompleted: console.log("v1", inst.modelData, "xa obj completed") } }
            LazyLoader { id: xb; loading: true; onLoadingChanged: console.log("v1", inst.modelData, "xb loading", loading); onActiveChanged: console.log("v1", inst.modelData, "xb active", active)
                QtObject { Component.onCompleted: console.log("v1", inst.modelData, "xb obj completed") } }
            LazyLoader { id: xc; active: true
                Scope { Component.onCompleted: console.log("v1", inst.modelData, "xc obj completed")
                    LazyLoader { active: true; QtObject { Component.onCompleted: console.log("v1", inst.modelData, "xc inner obj completed") } } } }
        } }
    Variants { id: v2; model: root.n
        Item { id: it; required property var modelData
            property var la: ya
            Component.onCompleted: console.log("v2 inst completed", modelData, ya.active)
            Item { LazyLoader { id: ya; active: true; QtObject { Component.onCompleted: console.log("v2", it.modelData, "ya obj completed") } } }
            LazyLoader { id: yb; active: true; QtObject { Component.onCompleted: console.log("v2", it.modelData, "yb obj completed") } }
        } }
    LazyLoader { id: outer; onActiveChanged: console.log("outer active", active)
        Scope { id: os; property var vv: v3
            Component.onCompleted: console.log("outer obj completed", v3.instances.length)
            LazyLoader { id: oa; active: true; QtObject { Component.onCompleted: console.log("outer oa obj completed") } }
            Variants { id: v3; model: ["p", "q"]
                Scope { id: i3; required property var modelData
                    property var la: za
                    Component.onCompleted: console.log("v3 inst completed", modelData)
                    LazyLoader { id: za; active: true; QtObject { Component.onCompleted: console.log("v3", i3.modelData, "za obj completed") } }
                    LazyLoader { id: zb; active: true; QtObject { Component.onCompleted: console.log("v3", i3.modelData, "zb obj completed") } }
                } }
            LazyLoader { id: oz; active: true; QtObject { Component.onCompleted: console.log("outer oz obj completed") } }
        } }
    LazyLoader { id: outer2
        Scope { property var vv: v4
            Variants { id: v4; model: ["r"]
                Scope { id: i4; required property var modelData
                    property var la: wa
                    LazyLoader { id: wa; active: true; QtObject { Component.onCompleted: console.log("v4", i4.modelData, "wa obj completed") } }
                } } } }
    property var steps: [
        ["state", () => console.log(v1.instances[0].la.active, v1.instances[0].lb.active, v2.instances[0].la.active)],
        ["v1 append", () => { root.m = ["a", "b"]; var i = v1.instances[1]; console.log("after", v1.instances.length, i.la.active, i.lb.loading, i.lb.active) }],
        ["state", () => { var i = v1.instances[1]; console.log(i.la.active, i.lb.loading, i.lb.active) }],
        ["v2 append", () => { root.n = ["x", "y"]; console.log("after", v2.instances.length, v2.instances[1].la.active) }],
        ["state", () => console.log(v2.instances[1].la.active)],
        ["outer.active=true", () => { outer.active = true; var v = outer.item.vv; console.log("after", v.instances.length, v.instances[0].la.active, v.instances[1].la.active) }],
        ["state", () => { var v = outer.item.vv; console.log(v.instances[0].la.active, v.instances[1].la.active) }],
        ["outer2.loading=true", () => { outer2.loading = true; console.log("after", outer2.active) }],
        ["state", () => { var v = outer2.item.vv; console.log(v.instances[0].la.active); v.model = ["r", "s"]; console.log("after model", v.instances.length, v.instances[1].la.active) }],
        ["state", () => console.log(outer2.item.vv.instances[1].la.active)]
    ]
    property int si: 0
    Timer { interval: 60; repeat: true; running: true; onTriggered: {
        if (root.si >= root.steps.length) { console.log("done"); running = false; Qt.quit(); return }
        console.log("--", root.steps[root.si][0]); root.steps[root.si][1](); root.si++
    } }
    Component.onCompleted: console.log("init", v1.instances.length, v1.instances[0].la.active, v2.instances[0].la.active)
}
