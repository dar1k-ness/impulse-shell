// `sourceSizeChanged` of an Image, as Qt 6.12 sends it. A write of another `sourceSize`, of one
// of its properties, through an alias or by a binding sends it; a write of the same value does
// not. A complete Image then loads again, and the end of every load sends it once more when the
// `sourceSize` is not the one at the end of the load before (which starts as none, so the
// first load always tells): two signals for a write, none for another `source` under a
// `sourceSize` that is set. An Image with no `source` does not load as it completes; an
// `asynchronous` one ends its load after the step. A `sourceSize` property an object sets
// (`sourceSize.width: 4`) is written with the whole value as the object is made, bindings to
// its properties follow one by one.
// Each step: the signals it sent, in order, then its value.
// The pictures do not exist (the checks load none): a load fails, and ends all the same.
// Not here: an Image with a `source` and a `sourceSize` dimension nothing set takes that
// dimension from the picture, so its load ends in the shell.
// qt: s g l f k m b m m k k f z y n nw w m l w="init";n nw n nw="10x3";="10x3";n nw n nw="3x4";="3x4";="3x4";="3x4";="3x4";n nw n nw="6x4";s s="4x3";="4x3";="4x3";g g g g="5x5";w w="9x3";="9x3";w w w w="2x2";y="9x3";="9x3";y y="8x3";z z="5x5";z z="0x0";b b="8x8";b b="9x8";="4x3";="4x3";f f="7x2";="4x2";m m="4x9";e e="6x6";="6x6";="6x6";u u="6x6";u u=0;n nw n nw=0;n nw n nw="2x2";
import QtQuick
Item {
    id: root
    property string result: ""
    property var log: []
    function ss(i) { return i.sourceSize.width + "x" + i.sourceSize.height }
    Image { id: s; sourceSize.width: 4; sourceSize.height: 2; onSourceSizeChanged: root.log.push("s") }
    Image { id: g; sourceSize { width: 4; height: 2 } onSourceSizeChanged: root.log.push("g") }
    Image { id: w; source: "nope-w.png"; sourceSize: Qt.size(4, 3); onSourceSizeChanged: root.log.push("w") }
    Image { id: n; sourceSize: Qt.size(4, 3); onSourceSizeChanged: root.log.push("n") }
    Image { id: y; asynchronous: true; source: "nope-y.png"; sourceSize: Qt.size(4, 3); onSourceSizeChanged: root.log.push("y") }
    Image { id: z; sourceSize: Qt.size(0, 0); onSourceSizeChanged: root.log.push("z") }
    Image { id: l; source: "nope-l.png"; sourceSize.width: 4; sourceSize.height: 3; onSourceSizeChanged: root.log.push("l") }
    Image { id: f; sourceSize.width: 2 + 2; sourceSize.height: 2; onSourceSizeChanged: root.log.push("f") }
    Image { id: k; sourceSize.width: 2 + 2; sourceSize.height: 1 + 1; onSourceSizeChanged: root.log.push("k") }
    Image { id: m; source: "nope-m.png"; sourceSize.width: 2 + 2; sourceSize.height: 1 + 1; onSourceSizeChanged: root.log.push("m") }
    Image { id: e; onSourceSizeChanged: root.log.push("e") }
    Image { id: u; onSourceSizeChanged: root.log.push("u") }
    property size held: Qt.size(7, 7)
    Image { id: b; sourceSize: root.held; onSourceSizeChanged: root.log.push("b") }
    property alias nw: n.sourceSize.width
    onNwChanged: log.push("nw")
    function step(f) {
        var v = f()
        result += log.join(" ") + "=" + JSON.stringify(v) + ";"
        log = []
    }
    Component.onCompleted: {
        step(() => "init")
        step(() => { n.sourceSize.width = 10; return ss(n) })
        step(() => { n.sourceSize.width = 10; return ss(n) })
        step(() => { n.sourceSize = Qt.size(3, 4); return ss(n) })
        step(() => { n.sourceSize = Qt.size(3, 4); return ss(n) })
        step(() => { n.source = "nope-n.png"; return ss(n) })
        step(() => { n.source = "nope-n.png"; return ss(n) })
        step(() => { n.source = ""; return ss(n) })
        step(() => { nw = 6; return ss(n) })
        step(() => { s.sourceSize.height = 3; return ss(s) })
        step(() => { s.source = "nope-s.png"; return ss(s) })
        step(() => { s.sourceSize = Qt.size(4, 3); return ss(s) })
        step(() => { g.sourceSize.width = 5; g.sourceSize.height = 5; return ss(g) })
        step(() => { w.sourceSize.width = 9; return ss(w) })
        step(() => { w.source = ""; return ss(w) })
        step(() => { w.sourceSize = Qt.size(1, 1); w.sourceSize = Qt.size(2, 2); return ss(w) })
        step(() => { y.sourceSize.width = 9; return ss(y) })
        step(() => { y.asynchronous = false; return ss(y) })
        step(() => { y.sourceSize.width = 8; return ss(y) })
        step(() => { z.sourceSize = Qt.size(5, 5); return ss(z) })
        step(() => { z.sourceSize = Qt.size(0, 0); return ss(z) })
        step(() => { held = Qt.size(8, 8); return ss(b) })
        step(() => { held.width = 9; return ss(b) })
        step(() => { l.source = "nope-l2.png"; return ss(l) })
        step(() => { l.sourceSize.width = 4; return ss(l) })
        step(() => { f.sourceSize.width = 7; return ss(f) })
        step(() => { k.sourceSize = Qt.size(4, 2); return ss(k) })
        step(() => { m.sourceSize.height = 9; return ss(m) })
        step(() => { e.sourceSize = Qt.size(6, 6); return ss(e) })
        step(() => { e.source = "nope-e.png"; return ss(e) })
        step(() => { e.source = ""; return ss(e) })
        step(() => { u.sourceSize = Qt.size(6, 6); return ss(u) })
        step(() => { u.sourceSize = undefined; return 0 })
        step(() => { n.sourceSize = undefined; return 0 })
        step(() => { n.sourceSize = Qt.size(2, 2); return ss(n) })
    }
}
