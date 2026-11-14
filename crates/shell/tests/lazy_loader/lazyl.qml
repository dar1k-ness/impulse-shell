import QtQuick
import Quickshell
Scope {
    id: root
    // how many times each binding was evaluated; changing a count notifies nothing
    property var n: ({ seen: 0, dep: 0, got: 0, f: 0 })
    function counts() { return "seen " + n.seen + " dep " + n.dep + " got " + n.got + " f " + n.f }
    property int k: 0
    LazyLoader { id: a; onItemChanged: console.log("a item", item !== null)
        QtObject { property int v: 5 } }
    // a binding that reads the item, evaluated again by k
    property int seen: { n.seen++; k; return a.item ? a.item.v : -1 }
    onSeenChanged: console.log("seen", seen)
    // a binding on that binding
    property int dep: { n.dep++; return seen * 2 }
    onDepChanged: console.log("dep", dep)
    // the first evaluation of a binding of an object being created reads the item
    LazyLoader { id: b; loading: true
        QtObject { property int v: 6 } }
    LazyLoader { id: c; active: true
        QtObject { property int got: { root.n.got++; return b.item ? b.item.v : -1 }
            Component.onCompleted: console.log("c obj completed", got) } }
    // a function called from a binding reads the item
    LazyLoader { id: d
        QtObject { property int v: 9 } }
    property int kf: 0
    function readD() { return d.item ? d.item.v : -1 }
    property int f: { n.f++; kf; return readD() }
    onFChanged: console.log("f", f)
    property var steps: [
        ["state", () => console.log(seen, dep, c.item.got, f, counts())],
        ["a.loading=true; k++", () => { a.loading = true; root.k++; console.log("after", seen, dep, counts()) }],
        ["state", () => console.log(seen, dep, counts())],
        ["a.active=false; k++", () => { a.active = false; root.k++; console.log("after", seen, dep, counts()) }],
        ["state", () => console.log(seen, dep, counts())],
        ["a.loading=true; k++ again", () => { a.loading = true; root.k++; console.log("after", seen, dep, counts()) }],
        ["state", () => console.log(seen, dep, counts())],
        ["a.active=false", () => { a.active = false; console.log("after", seen, dep, counts()) }],
        ["a.loading=true", () => { a.loading = true; console.log("after", seen, dep, counts()) }],
        ["state", () => console.log(seen, dep, counts())],
        ["d.loading=true; kf++", () => { d.loading = true; root.kf++; console.log("after", f, counts()) }],
        ["state", () => console.log(f, counts())],
        ["kf++", () => { root.kf++; console.log("after", f, counts()) }]
    ]
    property int si: 0
    Timer { interval: 60; repeat: true; running: true; onTriggered: {
        if (root.si >= root.steps.length) { console.log("done"); running = false; Qt.quit(); return }
        console.log("--", root.steps[root.si][0]); root.steps[root.si][1](); root.si++
    } }
    Component.onCompleted: console.log("init", seen, dep, f, counts())
}
