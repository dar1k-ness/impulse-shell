import QtQuick
import Quickshell
import Quickshell.Io
import "sub"
import "sub/lib.js" as Lib
import qs.mod
Scope {
    id: root
    Thing { id: thing }
    Widget { id: widget }
    Component { id: inline; Item {} }
    Image { id: i0; source: "big.png" }
    Image { id: i1; source: Qt.resolvedUrl("big.png") }
    Image { id: i2; source: "DIR/big.png" }
    Image { id: i3; source: "file://DIR/big.png" }
    Image { id: i4; source: "qs:@/qs/big.png" }
    Image { id: i5; source: "root:big.png" }
    Image { id: i6; source: "root:/big.png" }
    Image { id: i7; source: "qs:DIR/big.png" }
    Image { id: i8; source: "missing.png" }
    Image { id: i9; source: "" }
    Image { id: i10; source: "a[b].png" }
    Image { id: i11; source: Qt.resolvedUrl("a[b].png") }
    Loader { id: l0; source: "sub/Pic.qml" }
    Loader { id: l1; source: Qt.resolvedUrl("sub/Pic.qml") }
    Loader { id: l2; source: "DIR/sub/Pic.qml" }
    Loader { id: l3; source: "file://DIR/sub/Pic.qml" }
    Loader { id: l4; source: "qs:@/qs/sub/Pic.qml" }
    Loader { id: l5; source: "root:sub/Pic.qml" }
    Loader { id: l6; source: "root:/sub/Pic.qml" }
    Loader { id: l7; source: "qs:DIR/sub/Pic.qml" }
    Loader { id: l8; source: "sub/Missing.qml" }
    Loader { id: l9; source: "sub/sp%20ace.qml" }
    Loader { id: l10; source: "sub/sp ace.qml" }
    Loader { id: l11; source: "" }
    Loader { id: l12; source: "sub/P[i]c.qml" }
    Loader { id: l13; source: Qt.resolvedUrl("sub/P[i]c.qml") }
    FileView { id: f0; path: "note.txt"; blockLoading: true }
    FileView { id: f1; path: Qt.resolvedUrl("note.txt"); blockLoading: true }
    FileView { id: f2; path: "DIR/note.txt"; blockLoading: true }
    FileView { id: f3; path: "file://DIR/note.txt"; blockLoading: true }
    FileView { id: f4; path: "qs:@/qs/note.txt"; blockLoading: true }
    FileView { id: f5; path: "root:note.txt"; blockLoading: true }
    FileView { id: f6; path: "file://DIR/a%20b.txt"; blockLoading: true }
    FileView { id: f7; path: "DIR/a%20b.txt"; blockLoading: true }
    FileView { id: f8; path: "FILE://DIR/note.txt"; blockLoading: true }
    FileView { id: f9; path: "file:DIR/note.txt"; blockLoading: true }
    FileView { id: f10; path: "a[b]c.txt"; blockLoading: true }
    FileView { id: f11; path: Qt.resolvedUrl("a[b]c.txt"); blockLoading: true }
    FileView { id: f12; path: "DIR/a[b]c.txt"; blockLoading: true }
    FileView { id: f13; path: "file://DIR/a[b]c.txt"; blockLoading: true }
    FileView { id: f14; path: "file://DIR/a%5Bb%5Dc.txt"; blockLoading: true }
    LazyLoader { id: z0; source: "sub/Pic.qml"; loading: true }
    LazyLoader { id: z1; source: Qt.resolvedUrl("sub/Pic.qml"); loading: true }
    LazyLoader { id: z2; source: "DIR/sub/Pic.qml"; loading: true }
    LazyLoader { id: z3; source: "file://DIR/sub/Pic.qml"; loading: true }
    LazyLoader { id: z4; source: "qs:@/qs/sub/Pic.qml"; loading: true }
    LazyLoader { id: z5; source: "root:sub/Pic.qml"; loading: true }
    LazyLoader { id: z6; source: "root:/sub/Pic.qml"; loading: true }
    LazyLoader { id: z7; source: "qs:DIR/sub/Pic.qml"; loading: true }
    LazyLoader { id: z8; source: "sub/Missing.qml"; loading: true }
    function me(o) { return o ? o.me + " " + o.meq : "null" }
    function made(u) { var c = Qt.createComponent(u); var x = c.status === Component.Ready ? c.createObject(root) : null; return c.status + " " + c.url + " " + me(x) }
    function text(f) { try { return f.text() } catch (e) { return "throws" } }
    function report() {
        var o = [];
        function p(g, k, v) { o.push(g + " " + k + " => " + v) }
        p("R", "\"big.png\"", Qt.resolvedUrl("big.png"));
        p("R", "\"sub/pic.png\"", Qt.resolvedUrl("sub/pic.png"));
        p("R", "\"./big.png\"", Qt.resolvedUrl("./big.png"));
        p("R", "\"sub/../big.png\"", Qt.resolvedUrl("sub/../big.png"));
        p("R", "\"x.qml\"", Qt.resolvedUrl("x.qml"));
        p("R", "\"sub/Pic.qml\"", Qt.resolvedUrl("sub/Pic.qml"));
        p("R", "\"sub/\"", Qt.resolvedUrl("sub/"));
        p("R", "\"\"", Qt.resolvedUrl(""));
        p("R", "\"../up.png\"", Qt.resolvedUrl("../up.png"));
        p("R", "\"../up.qml\"", Qt.resolvedUrl("../up.qml"));
        p("R", "\"/abs/x.png\"", Qt.resolvedUrl("/abs/x.png"));
        p("R", "\"/abs/x.qml\"", Qt.resolvedUrl("/abs/x.qml"));
        p("R", "\"/abs/a%20b.png\"", Qt.resolvedUrl("/abs/a%20b.png"));
        p("R", "\"a b.png\"", Qt.resolvedUrl("a b.png"));
        p("R", "\"a b.qml\"", Qt.resolvedUrl("a b.qml"));
        p("R", "\"a%20b.qml\"", Qt.resolvedUrl("a%20b.qml"));
        p("R", "\"big.png?q=1\"", Qt.resolvedUrl("big.png?q=1"));
        p("R", "\"big.png#f\"", Qt.resolvedUrl("big.png#f"));
        p("R", "\"Pic.qml?q=1\"", Qt.resolvedUrl("Pic.qml?q=1"));
        p("R", "\"Pic.qml#f\"", Qt.resolvedUrl("Pic.qml#f"));
        p("R", "\"http://e/x.png\"", Qt.resolvedUrl("http://e/x.png"));
        p("R", "\"file:///b.png\"", Qt.resolvedUrl("file:///b.png"));
        p("R", "\"file:///b.qml\"", Qt.resolvedUrl("file:///b.qml"));
        p("R", "\"DIR/big.png\"", Qt.resolvedUrl("DIR/big.png"));
        p("R", "\"DIR/sub/Pic.qml\"", Qt.resolvedUrl("DIR/sub/Pic.qml"));
        p("R", "\"file://DIR/sub/Pic.qml\"", Qt.resolvedUrl("file://DIR/sub/Pic.qml"));
        p("R", "\"qs:@/qs/big.png\"", Qt.resolvedUrl("qs:@/qs/big.png"));
        p("R", "\"qs:@/qs/sub/Pic.qml\"", Qt.resolvedUrl("qs:@/qs/sub/Pic.qml"));
        p("R", "\"qs:/abs/x.png\"", Qt.resolvedUrl("qs:/abs/x.png"));
        p("R", "\"qs:/abs/x.qml\"", Qt.resolvedUrl("qs:/abs/x.qml"));
        p("R", "\"qs:big.png\"", Qt.resolvedUrl("qs:big.png"));
        p("R", "\"root:big.png\"", Qt.resolvedUrl("root:big.png"));
        p("R", "\"root:/big.png\"", Qt.resolvedUrl("root:/big.png"));
        p("R", "\"root:sub/Pic.qml\"", Qt.resolvedUrl("root:sub/Pic.qml"));
        p("R", "\"root:/sub/Pic.qml\"", Qt.resolvedUrl("root:/sub/Pic.qml"));
        p("R", "\"qrc:/x.png\"", Qt.resolvedUrl("qrc:/x.png"));
        p("R", "\"QS:@/qs/big.png\"", Qt.resolvedUrl("QS:@/qs/big.png"));
        p("R", "\"//host/x.png\"", Qt.resolvedUrl("//host/x.png"));
        p("R", "\"@/qs/big.png\"", Qt.resolvedUrl("@/qs/big.png"));
        p("R", "\".\"", Qt.resolvedUrl("."));
        p("R", "\"./\"", Qt.resolvedUrl("./"));
        p("R", "\"..\"", Qt.resolvedUrl(".."));
        p("R", "\"../\"", Qt.resolvedUrl("../"));
        p("R", "\"./.\"", Qt.resolvedUrl("./."));
        p("R", "\"sub/.\"", Qt.resolvedUrl("sub/."));
        p("R", "\"sub/./\"", Qt.resolvedUrl("sub/./"));
        p("R", "\"sub/..\"", Qt.resolvedUrl("sub/.."));
        p("R", "\"sub\"", Qt.resolvedUrl("sub"));
        p("R", "\"qs:@/qs/\"", Qt.resolvedUrl("qs:@/qs/"));
        p("R", "\"qs:@/qs/.\"", Qt.resolvedUrl("qs:@/qs/."));
        p("R", "\"qs:@/qs\"", Qt.resolvedUrl("qs:@/qs"));
        p("R", "\"root:\"", Qt.resolvedUrl("root:"));
        p("R", "\"root:.\"", Qt.resolvedUrl("root:."));
        p("R", "\"root:/\"", Qt.resolvedUrl("root:/"));
        p("R", "\"root:sub/\"", Qt.resolvedUrl("root:sub/"));
        p("R", "\"a[b]c.txt\"", Qt.resolvedUrl("a[b]c.txt"));
        p("R", "\"a[b]c.qml\"", Qt.resolvedUrl("a[b]c.qml"));
        p("R", "\"sub/a[b]c.txt\"", Qt.resolvedUrl("sub/a[b]c.txt"));
        p("R", "\"[\"", Qt.resolvedUrl("["));
        p("R", "\"]\"", Qt.resolvedUrl("]"));
        p("R", "\"/abs/a[b]c.png\"", Qt.resolvedUrl("/abs/a[b]c.png"));
        p("R", "\"/abs/a[b]c.qml\"", Qt.resolvedUrl("/abs/a[b]c.qml"));
        p("R", "\"file:///abs/a[b]c.png\"", Qt.resolvedUrl("file:///abs/a[b]c.png"));
        p("R", "\"qs:@/qs/a[b]c.png\"", Qt.resolvedUrl("qs:@/qs/a[b]c.png"));
        p("R", "\"qs:/abs/a[b]c.png\"", Qt.resolvedUrl("qs:/abs/a[b]c.png"));
        p("R", "\"root:a[b]c.png\"", Qt.resolvedUrl("root:a[b]c.png"));
        p("R", "\"a%5Bb%5Dc.txt\"", Qt.resolvedUrl("a%5Bb%5Dc.txt"));
        p("R", "\"a%5bb.txt\"", Qt.resolvedUrl("a%5bb.txt"));
        p("R", "\"a{b}c.txt\"", Qt.resolvedUrl("a{b}c.txt"));
        p("R", "\"a|b.txt\"", Qt.resolvedUrl("a|b.txt"));
        p("R", "\"a^b.txt\"", Qt.resolvedUrl("a^b.txt"));
        p("R", "\"a`b.txt\"", Qt.resolvedUrl("a`b.txt"));
        p("R", "\"a\\\"b.txt\"", Qt.resolvedUrl("a\"b.txt"));
        p("R", "\"a<b>c.txt\"", Qt.resolvedUrl("a<b>c.txt"));
        p("R", "\"a\\\\b.txt\"", Qt.resolvedUrl("a\\b.txt"));
        p("R", "\"a'b.txt\"", Qt.resolvedUrl("a'b.txt"));
        p("R", "\"a;b.txt\"", Qt.resolvedUrl("a;b.txt"));
        p("R", "\"a!$&()*+,=:@~c.txt\"", Qt.resolvedUrl("a!$&()*+,=:@~c.txt"));
        p("R", "\"a%25b.txt\"", Qt.resolvedUrl("a%25b.txt"));
        p("R", "\"a%2Fb.txt\"", Qt.resolvedUrl("a%2Fb.txt"));
        p("R", "\"big.png?[q]\"", Qt.resolvedUrl("big.png?[q]"));
        p("R", "\"big.png#[f]\"", Qt.resolvedUrl("big.png#[f]"));
        p("R", "\"\\u00e9[.txt\"", Qt.resolvedUrl("\u00e9[.txt"));
        p("R", "\"qs:@/qs//abs/x.png\"", Qt.resolvedUrl("qs:@/qs//abs/x.png"));
        p("R", "\"qs:@/qs/:x.png\"", Qt.resolvedUrl("qs:@/qs/:x.png"));
        p("R", "\"qs:@/qs/:/x.png\"", Qt.resolvedUrl("qs:@/qs/:/x.png"));
        p("R", "\"root://abs/x.png\"", Qt.resolvedUrl("root://abs/x.png"));
        p("R", "\"qs:@/qs//abs/X.qml\"", Qt.resolvedUrl("qs:@/qs//abs/X.qml"));
        p("R", "\"qs:@/qs/sub/\"", Qt.resolvedUrl("qs:@/qs/sub/"));
        p("R", "\"qs:@/qs/sub/.\"", Qt.resolvedUrl("qs:@/qs/sub/."));
        p("R", "\"qs:@/qs/./big.png\"", Qt.resolvedUrl("qs:@/qs/./big.png"));
        p("R", "\"qs:@/qs/../up.png\"", Qt.resolvedUrl("qs:@/qs/../up.png"));
        p("T", "\"pic.png\"", thing.f("pic.png"));
        p("W", "\"pic.png\"", widget.f("pic.png"));
        p("J", "\"pic.png\"", Lib.r("pic.png"));
        p("T", "\"Pic.qml\"", thing.f("Pic.qml"));
        p("W", "\"Pic.qml\"", widget.f("Pic.qml"));
        p("J", "\"Pic.qml\"", Lib.r("Pic.qml"));
        p("T", "\"../big.png\"", thing.f("../big.png"));
        p("W", "\"../big.png\"", widget.f("../big.png"));
        p("J", "\"../big.png\"", Lib.r("../big.png"));
        p("T", "\"../shell.qml\"", thing.f("../shell.qml"));
        p("W", "\"../shell.qml\"", widget.f("../shell.qml"));
        p("J", "\"../shell.qml\"", Lib.r("../shell.qml"));
        p("T", "\"../../up.png\"", thing.f("../../up.png"));
        p("W", "\"../../up.png\"", widget.f("../../up.png"));
        p("J", "\"../../up.png\"", Lib.r("../../up.png"));
        p("T", "\"\"", thing.f(""));
        p("W", "\"\"", widget.f(""));
        p("J", "\"\"", Lib.r(""));
        p("T", "\"/abs/x.png\"", thing.f("/abs/x.png"));
        p("W", "\"/abs/x.png\"", widget.f("/abs/x.png"));
        p("J", "\"/abs/x.png\"", Lib.r("/abs/x.png"));
        p("T", "\"/abs/x.qml\"", thing.f("/abs/x.qml"));
        p("W", "\"/abs/x.qml\"", widget.f("/abs/x.qml"));
        p("J", "\"/abs/x.qml\"", Lib.r("/abs/x.qml"));
        p("T", "\".\"", thing.f("."));
        p("W", "\".\"", widget.f("."));
        p("J", "\".\"", Lib.r("."));
        p("T", "\"./\"", thing.f("./"));
        p("W", "\"./\"", widget.f("./"));
        p("J", "\"./\"", Lib.r("./"));
        p("T", "\"..\"", thing.f(".."));
        p("W", "\"..\"", widget.f(".."));
        p("J", "\"..\"", Lib.r(".."));
        p("T", "\"../\"", thing.f("../"));
        p("W", "\"../\"", widget.f("../"));
        p("J", "\"../\"", Lib.r("../"));
        p("T", "\"../.\"", thing.f("../."));
        p("W", "\"../.\"", widget.f("../."));
        p("J", "\"../.\"", Lib.r("../."));
        p("T", "\"a[b]c.txt\"", thing.f("a[b]c.txt"));
        p("W", "\"a[b]c.txt\"", widget.f("a[b]c.txt"));
        p("J", "\"a[b]c.txt\"", Lib.r("a[b]c.txt"));
        p("T", "\"a[b]c.qml\"", thing.f("a[b]c.qml"));
        p("W", "\"a[b]c.qml\"", widget.f("a[b]c.qml"));
        p("J", "\"a[b]c.qml\"", Lib.r("a[b]c.qml"));
        p("T", "own", thing.own + " " + thing.inner.status + " " + thing.inner.source);
        p("W", "own", widget.own + " " + widget.inner.status + " " + widget.inner.source);
        p("W", "made", widget.mk("Leaf.qml"));
        p("X", "inline", inline.url + " " + inline.status);
        p("I", "\"big.png\"", i0.status + " " + i0.implicitWidth + " " + i0.source);
        p("I", "\"resolvedUrl(big.png)\"", i1.status + " " + i1.implicitWidth + " " + i1.source);
        p("I", "\"DIR/big.png\"", i2.status + " " + i2.implicitWidth + " " + i2.source);
        p("I", "\"file://DIR/big.png\"", i3.status + " " + i3.implicitWidth + " " + i3.source);
        p("I", "\"qs:@/qs/big.png\"", i4.status + " " + i4.implicitWidth + " " + i4.source);
        p("I", "\"root:big.png\"", i5.status + " " + i5.implicitWidth + " " + i5.source);
        p("I", "\"root:/big.png\"", i6.status + " " + i6.implicitWidth + " " + i6.source);
        p("I", "\"qs:DIR/big.png\"", i7.status + " " + i7.implicitWidth + " " + i7.source);
        p("I", "\"missing.png\"", i8.status + " " + i8.implicitWidth + " " + i8.source);
        p("I", "\"\"", i9.status + " " + i9.implicitWidth + " " + i9.source);
        p("I", "\"a[b].png\"", i10.status + " " + i10.implicitWidth + " " + i10.source);
        p("I", "\"resolvedUrl(a[b].png)\"", i11.status + " " + i11.implicitWidth + " " + i11.source);
        p("L", "\"sub/Pic.qml\"", l0.status + " " + l0.source + " " + me(l0.item));
        p("L", "\"resolvedUrl(sub/Pic.qml)\"", l1.status + " " + l1.source + " " + me(l1.item));
        p("L", "\"DIR/sub/Pic.qml\"", l2.status + " " + l2.source + " " + me(l2.item));
        p("L", "\"file://DIR/sub/Pic.qml\"", l3.status + " " + l3.source + " " + me(l3.item));
        p("L", "\"qs:@/qs/sub/Pic.qml\"", l4.status + " " + l4.source + " " + me(l4.item));
        p("L", "\"root:sub/Pic.qml\"", l5.status + " " + l5.source + " " + me(l5.item));
        p("L", "\"root:/sub/Pic.qml\"", l6.status + " " + l6.source + " " + me(l6.item));
        p("L", "\"qs:DIR/sub/Pic.qml\"", l7.status + " " + l7.source + " " + me(l7.item));
        p("L", "\"sub/Missing.qml\"", l8.status + " " + l8.source + " " + me(l8.item));
        p("L", "\"sub/sp%20ace.qml\"", l9.status + " " + l9.source + " " + me(l9.item));
        p("L", "\"sub/sp ace.qml\"", l10.status + " " + l10.source + " " + me(l10.item));
        p("L", "\"\"", l11.status + " " + l11.source + " " + me(l11.item));
        p("L", "\"sub/P[i]c.qml\"", l12.status + " " + l12.source + " " + me(l12.item));
        p("L", "\"resolvedUrl(sub/P[i]c.qml)\"", l13.status + " " + l13.source + " " + me(l13.item));
        p("C", "\"sub/Pic.qml\"", made("sub/Pic.qml"));
        p("C", "\"resolvedUrl(sub/Pic.qml)\"", made(Qt.resolvedUrl("sub/Pic.qml")));
        p("C", "\"DIR/sub/Pic.qml\"", made("DIR/sub/Pic.qml"));
        p("C", "\"file://DIR/sub/Pic.qml\"", made("file://DIR/sub/Pic.qml"));
        p("C", "\"qs:@/qs/sub/Pic.qml\"", made("qs:@/qs/sub/Pic.qml"));
        p("C", "\"root:sub/Pic.qml\"", made("root:sub/Pic.qml"));
        p("C", "\"root:/sub/Pic.qml\"", made("root:/sub/Pic.qml"));
        p("C", "\"qs:DIR/sub/Pic.qml\"", made("qs:DIR/sub/Pic.qml"));
        p("C", "\"sub/Missing.qml\"", made("sub/Missing.qml"));
        p("C", "\"sub/sp%20ace.qml\"", made("sub/sp%20ace.qml"));
        p("C", "\"sub/sp ace.qml\"", made("sub/sp ace.qml"));
        p("C", "\"sub/P[i]c.qml\"", made("sub/P[i]c.qml"));
        p("C", "\"resolvedUrl(sub/P[i]c.qml)\"", made(Qt.resolvedUrl("sub/P[i]c.qml")));
        p("F", "\"note.txt\"", f0.path + " " + JSON.stringify(text(f0)));
        p("F", "\"resolvedUrl(note.txt)\"", f1.path + " " + JSON.stringify(text(f1)));
        p("F", "\"DIR/note.txt\"", f2.path + " " + JSON.stringify(text(f2)));
        p("F", "\"file://DIR/note.txt\"", f3.path + " " + JSON.stringify(text(f3)));
        p("F", "\"qs:@/qs/note.txt\"", f4.path + " " + JSON.stringify(text(f4)));
        p("F", "\"root:note.txt\"", f5.path + " " + JSON.stringify(text(f5)));
        p("F", "\"file://DIR/a%20b.txt\"", f6.path + " " + JSON.stringify(text(f6)));
        p("F", "\"DIR/a%20b.txt\"", f7.path + " " + JSON.stringify(text(f7)));
        p("F", "\"FILE://DIR/note.txt\"", f8.path + " " + JSON.stringify(text(f8)));
        p("F", "\"file:DIR/note.txt\"", f9.path + " " + JSON.stringify(text(f9)));
        p("F", "\"a[b]c.txt\"", f10.path + " " + JSON.stringify(text(f10)));
        p("F", "\"resolvedUrl(a[b]c.txt)\"", f11.path + " " + JSON.stringify(text(f11)));
        p("F", "\"DIR/a[b]c.txt\"", f12.path + " " + JSON.stringify(text(f12)));
        p("F", "\"file://DIR/a[b]c.txt\"", f13.path + " " + JSON.stringify(text(f13)));
        p("F", "\"file://DIR/a%5Bb%5Dc.txt\"", f14.path + " " + JSON.stringify(text(f14)));
        p("Z", "\"sub/Pic.qml\"", z0.source + " " + me(z0.item));
        p("Z", "\"resolvedUrl(sub/Pic.qml)\"", z1.source + " " + me(z1.item));
        p("Z", "\"DIR/sub/Pic.qml\"", z2.source + " " + me(z2.item));
        p("Z", "\"file://DIR/sub/Pic.qml\"", z3.source + " " + me(z3.item));
        p("Z", "\"qs:@/qs/sub/Pic.qml\"", z4.source + " " + me(z4.item));
        p("Z", "\"root:sub/Pic.qml\"", z5.source + " " + me(z5.item));
        p("Z", "\"root:/sub/Pic.qml\"", z6.source + " " + me(z6.item));
        p("Z", "\"qs:DIR/sub/Pic.qml\"", z7.source + " " + me(z7.item));
        p("Z", "\"sub/Missing.qml\"", z8.source + " " + me(z8.item));
        return o;
    }
    function reportText() { return report().join("\n") }
}
