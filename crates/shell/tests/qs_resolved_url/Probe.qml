import QtQuick
import Rustshell
import Rustshell.Io
import "sub"
import ri.mod
Scope {
    id: root
    Thing { id: thing }
    Widget { id: widget }
    QtObject { id: plain }
    Component { id: picture; Image {} }
    Component { id: holder; QtObject { property url u; property string s; property var v } }
    Loader { id: loader }
    FontLoader { id: font }
    Component { id: viewer; FileView { blockLoading: true } }
    property url u
    property var contexts: [["root", root], ["thing", thing], ["widget", widget], ["thing.inner", thing.inner],
        ["plain", plain], ["null", null], ["undefined", undefined]]
    property var urls: ["big.png", "x.qml", "sub/Pic.qml", "../up.png", "../up.qml", "../shell.qml", "/abs/x.png",
        "/abs/x.qml", "", "a b.png", "qs:@/qs/big.png", "qs:@/qs/sub/Pic.qml", "root:big.png", "qrc:/x.png",
        "http://e/x.png", "file://DIR/big.png", "DIR/sub/Pic.qml", "file:///b.qml"]
    function attempt(f) { try { var v = f(); return typeof v + " " + v } catch (e) { return "throws " + e } }
    // What is read after the files are: an Image's status and size, a FileView's text.
    property var later: []
    function dynamic(...at) {
        return Qt.createQmlObject("import QtQuick; Image { source: 'pic.png'; property string me: Qt.resolvedUrl('x.png');"
            + " property string q: Qt.resolvedUrl('x.qml') }", root, ...at);
    }
    function setup() {
        function q(k, ...at) {
            try { var x = dynamic(...at); later.push(["Q", k, () => x.source + " " + x.me + " " + x.q + " " + x.status]) }
            catch (e) { later.push(["Q", k, () => "throws " + e]) }
        }
        q("none");
        q("\"sub/dyn.qml\"", "sub/dyn.qml");
        q("resolvedUrl(sub/dyn.qml)", Qt.resolvedUrl("sub/dyn.qml"));
        q("url(sub/dyn.qml)", Qt.url("sub/dyn.qml"));
        q("\"qs:@/qs/sub/dyn.qml\"", "qs:@/qs/sub/dyn.qml");
        q("\"DIR/sub/dyn.qml\"", "DIR/sub/dyn.qml");
        q("\"file://DIR/sub/dyn.qml\"", "file://DIR/sub/dyn.qml");
        q("resolvedUrl(dyn.qml, thing)", Qt.resolvedUrl("dyn.qml", thing));
        q("\"\"", "");
        q("1", 1);
        for (let v of ["big.png", Qt.resolvedUrl("big.png"), Qt.url("big.png"), Qt.resolvedUrl("pic.png", thing),
                Qt.resolvedUrl(Qt.resolvedUrl("big.png")), "qs:@/qs/big.png", Qt.url("qs:@/qs/big.png"), Qt.url("root:big.png")]) {
            let k = typeof v + " " + v;
            let made = picture.createObject(root, {source: v});
            later.push(["C", k, () => made.source + " " + made.status + " " + made.implicitWidth]);
            let set = picture.createObject(root);
            set.source = v;
            later.push(["I", k, () => set.source + " " + set.status + " " + set.implicitWidth]);
        }
        for (let v of ["note.txt", Qt.resolvedUrl("note.txt"), Qt.url("DIR/note.txt"), Qt.resolvedUrl(Qt.resolvedUrl("note.txt")),
                Qt.resolvedUrl("note.txt", thing)]) {
            let view = viewer.createObject(root);
            view.path = v;
            later.push(["V", typeof v + " " + v, () => view.path + " " + JSON.stringify(view.text())]);
        }
    }
    Component.onCompleted: setup()
    function report() {
        var o = [];
        function p(g, k, v) { o.push(g + " " + k + " => " + v) }
        for (var [name, c] of contexts) {
            for (var s of urls) {
                p("S " + name, JSON.stringify(s), attempt(() => Qt.resolvedUrl(s, c)));
                p("U " + name, JSON.stringify(s), attempt(() => Qt.resolvedUrl(Qt.url(s), c)));
            }
        }
        for (var s of urls) {
            p("S", JSON.stringify(s), attempt(() => Qt.resolvedUrl(s)));
            p("U", JSON.stringify(s), attempt(() => Qt.resolvedUrl(Qt.url(s))));
            p("RR", JSON.stringify(s), attempt(() => Qt.resolvedUrl(Qt.resolvedUrl(s))));
            p("RR thing", JSON.stringify(s), attempt(() => Qt.resolvedUrl(Qt.resolvedUrl(s), thing)));
            p("R thing R", JSON.stringify(s), attempt(() => Qt.resolvedUrl(Qt.resolvedUrl(s, thing))));
            p("T", JSON.stringify(s), attempt(() => thing.f(s)));
            p("T root", JSON.stringify(s), attempt(() => thing.f(s, root)));
            p("T widget", JSON.stringify(s), attempt(() => thing.f(Qt.url(s), widget)));
            p("W thing", JSON.stringify(s), attempt(() => widget.f(s, thing)));
        }
        p("X", "1", attempt(() => Qt.resolvedUrl("big.png", 1)));
        p("X", "\"s\"", attempt(() => Qt.resolvedUrl("big.png", "s")));
        p("X", "{}", attempt(() => Qt.resolvedUrl("big.png", {})));
        p("X", "Qt", attempt(() => Qt.resolvedUrl("big.png", Qt)));
        p("X", "three", attempt(() => Qt.resolvedUrl("big.png", thing, 3)));
        for (var [g, k, f] of later)
            p(g, k, attempt(f));
        for (var v of ["big.png", Qt.resolvedUrl("big.png"), Qt.url("big.png"), Qt.resolvedUrl("pic.png", thing),
                Qt.resolvedUrl(Qt.resolvedUrl("big.png")), "qs:@/qs/big.png", Qt.url("qs:@/qs/big.png"), Qt.url("root:big.png")]) {
            var k = typeof v + " " + v;
            p("H", k, attempt(() => { var x = holder.createObject(root, {u: v, s: v, v: v}); return x.u + " " + x.s + " " + typeof x.v + " " + x.v }));
            p("P", k, attempt(() => { root.u = v; return root.u }));
            p("F", k, attempt(() => { font.source = v; return font.source }));
        }
        for (var v of ["sub/Pic.qml", Qt.resolvedUrl("sub/Pic.qml"), Qt.url("sub/Pic.qml"), Qt.resolvedUrl("Pic.qml", thing),
                Qt.resolvedUrl(Qt.resolvedUrl("sub/Pic.qml")), Qt.url("qs:@/qs/sub/Pic.qml")]) {
            p("L", typeof v + " " + v, attempt(() => { loader.source = v; return loader.source + " " + loader.status + " " + (loader.item ? loader.item.me + " " + loader.item.meq : null) }));
        }
        return o;
    }
    function reportText() { return report().join("\n") }
}
