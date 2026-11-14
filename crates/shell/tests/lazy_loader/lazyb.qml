import QtQuick
import Quickshell
Scope {
    id: root
    property int k: 0
    LazyLoader { id: b1; onActiveChanged: console.log("b1 active", active, loading); onLoadingChanged: console.log("b1 loading", loading, active); onItemChanged: console.log("b1 item", item !== null)
        QtObject { property int v: 5; Component.onCompleted: console.log("b1 obj completed")
            Component.onDestruction: console.log("b1 obj destroyed") } }
    property int seen: { k; return b1.item ? b1.item.v : -1 }
    onSeenChanged: console.log("seen", seen)
    // an asynchronous load that a later loader's object reads in a binding while it is created
    LazyLoader { id: b2; loading: true; onActiveChanged: console.log("b2 active", active, loading); onLoadingChanged: console.log("b2 loading", loading, active)
        QtObject { property int v: 6; Component.onCompleted: console.log("b2 obj completed") } }
    LazyLoader { id: b3; active: true
        QtObject { property int got: b2.item ? b2.item.v : -1; Component.onCompleted: console.log("b3 obj completed", got) } }
    // a binding that follows `loading` and reads the item
    LazyLoader { id: b4; onActiveChanged: console.log("b4 active", active, loading); onLoadingChanged: console.log("b4 loading", loading, active)
        QtObject { property int v: 7; Component.onCompleted: console.log("b4 obj completed") } }
    property int viaLoading: b4.loading ? (b4.item ? b4.item.v : -2) : -1
    onViaLoadingChanged: console.log("viaLoading", viaLoading)
    // a binding that reads only whether there is an item
    LazyLoader { id: b5; onActiveChanged: console.log("b5 active", active, loading)
        QtObject { Component.onCompleted: console.log("b5 obj completed") } }
    property int k5: 0
    property bool has5: { k5; return b5.item !== null }
    onHas5Changed: console.log("has5", has5)
    // a function called from a binding
    LazyLoader { id: b6; onActiveChanged: console.log("b6 active", active, loading)
        QtObject { property int v: 9; Component.onCompleted: console.log("b6 obj completed") } }
    property int k6: 0
    function read6() { return b6.item ? b6.item.v : -1 }
    property int f6: { k6; return read6() }
    onF6Changed: console.log("f6", f6)
    // a handler of the loadingChanged that starts the load reads the item
    LazyLoader { id: b7; onLoadingChanged: console.log("b7 loading", loading, item !== null, active)
        QtObject { Component.onCompleted: console.log("b7 obj completed") } }
    property var steps: [
        ["state", () => console.log("seen", seen, "b2", b2.active, b2.loading, "b3", b3.item.got, "viaLoading", viaLoading)],
        ["b1.loading=true; k++", () => { b1.loading = true; console.log("now", b1.active, b1.loading); root.k++; console.log("after", seen, b1.active, b1.loading) }],
        ["state", () => console.log("seen", seen, b1.active, b1.loading)],
        ["b1.active=false; k++", () => { b1.active = false; root.k++; console.log("after", seen, b1.active, b1.loading) }],
        ["b1.loading=true", () => { b1.loading = true; console.log("after", seen, b1.active, b1.loading) }],
        ["state", () => console.log("seen", seen, b1.active, b1.loading)],
        ["b4.loading=true", () => { b4.loading = true; console.log("after", viaLoading, b4.active, b4.loading) }],
        ["state", () => console.log("viaLoading", viaLoading, b4.active, b4.loading)],
        ["b5.loading=true; k5++", () => { b5.loading = true; root.k5++; console.log("after", has5, b5.active, b5.loading) }],
        ["state", () => console.log("has5", has5, b5.active, b5.loading)],
        ["b6.loading=true; k6++", () => { b6.loading = true; root.k6++; console.log("after", f6, b6.active, b6.loading) }],
        ["state", () => console.log("f6", f6, b6.active, b6.loading)],
        ["b7.loading=true", () => { b7.loading = true; console.log("after", b7.active, b7.loading) }],
        ["state", () => console.log("b7", b7.active, b7.loading)]
    ]
    property int si: 0
    Timer { interval: 60; repeat: true; running: true; onTriggered: {
        if (root.si >= root.steps.length) { console.log("done"); running = false; Qt.quit(); return }
        console.log("--", root.steps[root.si][0]); root.steps[root.si][1](); root.si++
    } }
    Component.onCompleted: console.log("init seen", seen, "b2", b2.active, b2.loading, "viaLoading", viaLoading)
}
