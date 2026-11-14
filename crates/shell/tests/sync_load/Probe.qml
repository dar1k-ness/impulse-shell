import QtQuick
// What is ready in the call that writes an address, and what only after the event loop turned:
// each object logs its own change signals, read once in that call and once when nothing loads.
Item {
    id: root
    property var out: []
    property var watched: []
    property int pending: 0
    property var waiting: []
    property string report: ""
    signal finished()
    readonly property string font: "/usr/share/fonts/noto/NotoSansGothic-Regular.ttf"

    readonly property string pictureLog: "property var log: []\n"
        + "onStatusChanged: log.push('status=' + status)\n"
        + "onSourceSizeChanged: log.push('sourceSize=' + sourceSize.width + 'x' + sourceSize.height)\n"
        + "onImplicitWidthChanged: log.push('implicitWidth=' + implicitWidth)\n"
        + "onImplicitHeightChanged: log.push('implicitHeight=' + implicitHeight)\n"
        + "onProgressChanged: (p) => log.push('progress=' + p)\n"
        + "onSourceChanged: log.push('source')\n"
    readonly property string paintedLog: "onPaintedGeometryChanged: log.push('painted=' + paintedWidth + 'x' + paintedHeight)\n"
    readonly property string loaderLog: "property var log: []\n"
        + "onStatusChanged: log.push('status=' + status)\n"
        + "onItemChanged: log.push('item=' + (item ? item.tag : item))\n"
        + "onLoaded: log.push('loaded')\n"
        + "onProgressChanged: log.push('progress=' + progress)\n"
        + "onSourceChanged: log.push('source')\n"
        + "onSourceComponentChanged: log.push('sourceComponent')\n"
        + "onActiveChanged: log.push('active=' + active)\n"
    readonly property string fontLog: "property var log: []\n"
        + "onStatusChanged: log.push('status=' + status)\n"
        + "onNameChanged: log.push('name=' + name)\n"
        + "onSourceChanged: log.push('source')\n"

    function picture(x) {
        return x.status + " sourceSize " + x.sourceSize.width + "x" + x.sourceSize.height + " implicit " + x.implicitWidth
            + "x" + x.implicitHeight + " progress " + x.progress
    }
    function painted(x) { return picture(x) + " painted " + x.paintedWidth + "x" + x.paintedHeight }
    function loader(x) { return x.status + " item " + (x.item ? x.item.tag : x.item) + " progress " + x.progress }
    function fontState(x) { return x.status + " name " + JSON.stringify(x.name) }
    function component(x) { return x.status + " progress " + x.progress + " " + tidy(x.errorString()) }
    function tidy(s) { return String(s).split(Qt.resolvedUrl(".")).join("DIR/").trim().split("\n").join(" | ") }

    function make(type, body, log) { return Qt.createQmlObject("import QtQuick\n" + type + " {\n" + log + body + "\n}", root) }
    // The object's state and the signals it has sent since it was last read, now and when
    // nothing is loading. `owner` is who has `status` when the log is kept beside the object.
    function watch(name, x, state, act, owner) {
        var w = { name: name, x: x, state: state, seen: act ? x.log.length : 0 }
        var said = act ? attempt(act) : ""
        line(w, "same call", said)
        watched.push(w)
        var o = owner || x
        if (o.status === 2)
            hold(x, [o.statusChanged], () => o.status !== 2)
        return x
    }
    // Nothing is read for the last time before `x` says, by one of `signals`, that it is `done`.
    function hold(x, signals, done) {
        if (waiting.indexOf(x) >= 0)
            return
        waiting.push(x)
        pending += 1
        var heard = function() {
            var k = waiting.indexOf(x)
            if (k >= 0 && done()) {
                waiting.splice(k, 1)
                pending -= 1
                if (pending === 0)
                    Qt.callLater(root.finish)
            }
        }
        for (var signal of signals)
            signal.connect(heard)
    }
    function line(w, when, said) {
        var log = w.x.log
        var state
        try { state = w.state(w.x) } catch (e) { state = "throws " + tidy(e) }
        out.push(w.name + " | " + when + " | " + state + " | " + log.slice(w.seen).join(",") + (said ? " | " + said : ""))
        w.seen = log.length
    }
    function attempt(f) { try { var v = f(); return v === undefined ? "" : "returns " + v } catch (e) { return "throws " + tidy(e) } }
    // What a type built on this one reads as well.
    function extra() {}
    function part(name, f) { try { f() } catch (e) { out.push(name + " | stopped: " + tidy(e)) } }
    // What a reader that gave up waiting gets.
    function soFar() {
        var names = watched.filter(w => waiting.indexOf(w.x) >= 0).map(w => w.name)
        return out.join("\n") + "\nnever finished, still loading: " + names.filter((n, k) => names.indexOf(n) === k).join("; ")
    }
    function finish() {
        // A load that ended while others were still being started does not end the reading.
        if (pending !== 0 || report !== "")
            return
        // One line an object, from where its last read left off.
        for (var k = 0; k < watched.length; k++) {
            var w = watched[k]
            if (!watched.slice(k + 1).some(v => v.x === w.x))
                line(w, "later", "")
        }
        report = out.join("\n")
        finished()
    }

    function pictures(type, state, log, ext, n, sized) {
        var f = k => n[k] + "." + ext
        watch(type + " declared", make(type, "source: '" + f(0) + "'", log), state)
        var x = make(type, "", log)
        watch(type + " with no source", x, state)
        watch(type + " written", x, state, () => { x.source = f(1) })
        watch(type + " declared asynchronous", make(type, "asynchronous: true; source: '" + f(2) + "'", log), state)
        var y = make(type, "asynchronous: true", log)
        watch(type + " written asynchronous", y, state, () => { y.source = f(3) })
        var z = make(type, "asynchronous: true", log)
        watch(type + " written asynchronous, a file loaded before", z, state, () => { z.source = f(0) })
        watch(type + " declared, no such file", make(type, "source: 'none-" + type + "." + ext + "'", log), state)
        var m = make(type, "", log)
        watch(type + " written, no such file", m, state, () => { m.source = "none2-" + type + "." + ext })
        var ma = make(type, "asynchronous: true", log)
        watch(type + " written asynchronous, no such file", ma, state, () => { ma.source = "none3-" + type + "." + ext })
        var t = make(type, "", log)
        watch(type + " written, not a picture", t, state, () => { t.source = "text.png" })
        var u = make(type, "", log)
        watch(type + " written, a url value", u, state, () => { u.source = Qt.resolvedUrl(f(4)) })
        if (sized) {
            var s = make(type, "sourceSize: Qt.size(2, 2)", log)
            watch(type + " written, sourceSize 2x2", s, state, () => { s.source = f(5) })
            var sw = make(type, "sourceSize.width: 8", log)
            watch(type + " written, sourceSize.width 8", sw, state, () => { sw.source = f(5) })
        }
        var r = make(type, "source: '" + f(6) + "'", log)
        watch(type + " written over a loaded one", r, state, () => { r.source = f(7) })
        watch(type + " written empty over a loaded one", r, state, () => { r.source = "" })
        var ra = make(type, "asynchronous: true; source: '" + f(8) + "'", log)
        watch(type + " written over a loading one", ra, state, () => { ra.source = f(9) })
        var h = make(type, "visible: false", log)
        watch(type + " written, not visible", h, state, () => { h.source = f(10) })
        var late = make(type, "source: '" + f(11) + "'", log)
        watch(type + " made asynchronous after it loaded", late, state, () => { late.asynchronous = true })
        if (sized) {
            var sz = make(type, "source: '" + f(12) + "'", log)
            watch(type + " sourceSize written after it loaded", sz, state, () => { sz.sourceSize = Qt.size(2, 2) })
        }
    }
    function loaders() {
        var L = (body) => make("Loader", body, loaderLog)
        watch("Loader declared", L("source: 'Leaf.qml'"), loader)
        var a = L("")
        watch("Loader written", a, loader, () => { a.source = "Leaf.qml" })
        watch("Loader written again, the same", a, loader, () => { a.source = "Leaf.qml" })
        watch("Loader written again, another", a, loader, () => { a.source = "Other.qml" })
        watch("Loader written empty", a, loader, () => { a.source = "" })
        watch("Loader declared asynchronous", L("asynchronous: true; source: 'Leaf2.qml'"), loader)
        var b = L("asynchronous: true")
        watch("Loader written asynchronous", b, loader, () => { b.source = "Leaf3.qml" })
        watch("Loader declared, no such file", L("source: 'None.qml'"), loader)
        var c = L("")
        watch("Loader written, no such file", c, loader, () => { c.source = "None2.qml" })
        var d = L("")
        watch("Loader written, a file with an error", d, loader, () => { d.source = "parse_errors/Bad.qml" })
        var e = L("active: false; source: 'Leaf.qml'")
        watch("Loader not active", e, loader)
        watch("Loader made active", e, loader, () => { e.active = true })
        watch("Loader made inactive", e, loader, () => { e.active = false })
        watch("Loader sourceComponent", L("sourceComponent: Component { Item { property string tag: 'inline' } }"), loader)
        var u = L("")
        watch("Loader written, a url value", u, loader, () => { u.source = Qt.resolvedUrl("Leaf.qml") })

        var s = L("")
        watch("setSource(url)", s, loader, () => s.setSource("Leaf.qml"))
        watch("setSource(url) again, the same", s, loader, () => s.setSource("Leaf.qml"))
        watch("setSource(url, {tag}) over it", s, loader, () => s.setSource("Leaf.qml", { tag: "given" }))
        watch("setSource(url, {tag}) again, the same", s, loader, () => s.setSource("Leaf.qml", { tag: "given" }))
        watch("setSource(url, {tag}) again, another tag", s, loader, () => s.setSource("Leaf.qml", { tag: "second" }))
        watch("setSource(another url)", s, loader, () => s.setSource("Other.qml"))
        watch("source read after setSource", s, loader, () => tidy(s.source))
        watch("setSource(\"\")", s, loader, () => s.setSource(""))
        var p = L("")
        watch("setSource(url value, {tag})", p, loader, () => p.setSource(Qt.resolvedUrl("Leaf.qml"), { tag: "by url" }))
        var q = L("asynchronous: true")
        watch("setSource(url, {tag}) asynchronous", q, loader, () => q.setSource("Leaf4.qml", { tag: "later" }))
        var n = L("")
        watch("setSource, no such file", n, loader, () => n.setSource("None3.qml", { tag: "x" }))
        var bad = L("")
        watch("setSource, a file with an error", bad, loader, () => bad.setSource("parse_errors/Bad.qml"))
        var w = L("")
        watch("setSource()", w, loader, () => w.setSource())
        watch("setSource(1)", w, loader, () => w.setSource(1))
        watch("setSource(null)", w, loader, () => w.setSource(null))
        watch("setSource(undefined)", w, loader, () => w.setSource(undefined))
        watch("setSource(url, 1)", w, loader, () => w.setSource("Leaf.qml", 1))
        watch("setSource(url, null)", w, loader, () => w.setSource("Leaf.qml", null))
        watch("setSource(url, {}, 3)", w, loader, () => w.setSource("Leaf.qml", {}, 3))
        var k = L("")
        watch("setSource(url, {unknown})", k, loader, () => k.setSource("Leaf.qml", { nothing: 1 }))
        var i = L("active: false")
        watch("setSource(url, {tag}) while not active", i, loader, () => i.setSource("Leaf.qml", { tag: "kept" }))
        watch("made active after setSource", i, loader, () => { i.active = true })
        var o = L("source: 'Other.qml'")
        watch("setSource(url, {tag}) over a declared source", o, loader, () => o.setSource("Leaf.qml", { tag: "over" }))
        watch("source written over setSource", o, loader, () => { o.source = "Leaf2.qml" })
    }
    function components() {
        // A Component made by Qt.createComponent has no handlers of ours: its signals are heard from outside.
        function one(name, f) {
            var c
            try { c = f() } catch (e) { out.push(name + " | throws " + e); return }
            var x = { log: [], c: c }
            c.statusChanged.connect(() => x.log.push("status=" + c.status))
            c.progressChanged.connect(() => x.log.push("progress=" + c.progress))
            watch(name, x, v => component(v.c), null, c)
        }
        one("createComponent(url)", () => Qt.createComponent("Leaf.qml"))
        one("createComponent(url, Asynchronous)", () => Qt.createComponent("Leaf2.qml", Component.Asynchronous))
        one("createComponent(a file not loaded before, Asynchronous)", () => Qt.createComponent("Fresh.qml", Component.Asynchronous))
        one("createComponent(no such file)", () => Qt.createComponent("None4.qml"))
        one("createComponent(no such file, Asynchronous)", () => Qt.createComponent("None5.qml", Component.Asynchronous))
        one("createComponent(a file with an error)", () => Qt.createComponent("parse_errors/Bad.qml"))
        one("createComponent(url value)", () => Qt.createComponent(Qt.resolvedUrl("Leaf.qml")))
    }
    function fonts() {
        var F = body => make("FontLoader", body, fontLog)
        watch("FontLoader declared", F("source: '" + font + "'"), fontState)
        var a = F("")
        watch("FontLoader written", a, fontState, () => { a.source = font })
        watch("FontLoader written again, the same", a, fontState, () => { a.source = font })
        watch("FontLoader declared, no such file", F("source: 'none.ttf'"), fontState)
        var b = F("")
        watch("FontLoader written, no such file", b, fontState, () => { b.source = "none2.ttf" })
        var c = F("")
        watch("FontLoader written, not a font", c, fontState, () => { c.source = "note.txt" })
        var d = F("")
        watch("FontLoader written, a url value", d, fontState, () => { d.source = Qt.url("file://" + font) })
    }
    Image { id: still; source: "t.png"; property var log: [] }
    Component { id: made; Image { property var log: []; onStatusChanged: log.push("status=" + status) } }

    Component.onCompleted: {
        var files = p => [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12].map(k => p + k)
        part("pictures", () => pictures("Image", painted, pictureLog + paintedLog, "png", files("i"), true))
        watch("Image of the document, in its Component.onCompleted", still, painted)
        watch("Image by createObject(root, {source})", made.createObject(root, { source: "n.png" }), painted)
        watch("Image by createObject(root, {source, asynchronous})", made.createObject(root, { source: "o.png", asynchronous: true }), painted)
        watch("Image by createObject(root, {asynchronous, source})", made.createObject(root, { asynchronous: true, source: "p.png" }), painted)
        var nc = make("Image", "cache: false", pictureLog + paintedLog)
        watch("Image written, cache: false", nc, painted, () => { nc.source = "i0.png" })
        var remote = make("Image", "", pictureLog + paintedLog)
        watch("Image written, http", remote, painted, () => { remote.source = "http://127.0.0.1:9/x.png" })
        part("pictures", () => pictures("BorderImage", picture, pictureLog, "png", files("b")))
        part("pictures", () => pictures("AnimatedImage", painted, pictureLog + paintedLog, "gif", files("g"), true))
        part("loaders", loaders)
        part("components", components)
        part("fonts", fonts)
        extra()
        if (pending === 0)
            Qt.callLater(root.finish)
    }
}
