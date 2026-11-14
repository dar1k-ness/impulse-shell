// Qt's lines are those of a run without the disk cache (QML_DISABLE_DISK_CACHE=1): the number
// in `Class_QML_N` is taken per object when the file is compiled, and a cached unit takes one
// more per Component.
import QtQuick
Item {
    id: root
    property int a: 1
    Rectangle { id: plain }
    Rectangle { id: withProp; property int b: 2 }
    Rectangle { id: withSig; signal s() }
    Rectangle { id: withFn; function f() {} }
    Rectangle { id: named; objectName: "nm"; property int c: 0 }
    Rectangle { id: namedPlain; objectName: "a \"b\" \\ c" }
    Rectangle { id: handlerOnly; onWidthChanged: {} }
    Rectangle { id: withOn; NumberAnimation on x { to: 1 } }
    Rectangle { id: withAlias; property alias al: plain.width }
    Rectangle { id: withEnum; enum E { A, B } }
    Rectangle { id: withGroup; border.width: 2; anchors.margins: 1 }
    Rectangle { id: withBehavior; Behavior on x { NumberAnimation {} } }
    Component { id: comp; Rectangle { property int k: 1 } }
    Component { id: comp2; Rectangle { } }
    QtObject { id: qo; property int v: 1 }
    QtObject { id: qoPlain }
    Timer { id: tm }
    Text { id: txt; text: "t" }
    Loader { id: ld; sourceComponent: comp }
    Repeater { id: rp; model: 2; Item { property int idx: index } }
    Component { id: lvc; QtObject { property list<var> lv } }
    function t(name, f) { var s; try { s = String(f()) } catch (e) { s = "THROW " + e.name + ": " + e.message } console.log(name + " => " + s.replace(/\n/g, "\\n")) }
    function d(o, k) { var p = Object.getOwnPropertyDescriptor(o, k); return p ? "w=" + p.writable + " e=" + p.enumerable + " c=" + p.configurable + " v=" + typeof p.value : "none" }
    function e(o, k) { var p = Object.getOwnPropertyDescriptor(o, k); return p ? "w=" + p.writable + " e=" + p.enumerable + " c=" + p.configurable : "none" }
    Component.onCompleted: {
        var all = { root, plain, withProp, withSig, withFn, named, namedPlain, handlerOnly, withOn, withAlias, withEnum, withGroup, withBehavior, comp, comp2, qo, qoPlain, tm, txt, ld, rp }
        for (var k in all) { let o = all[k]; t("s " + k, function() { return "" + o }) }
        t("s loaded", function() { return "" + ld.item })
        t("s rep0", function() { return "" + rp.itemAt(0) })
        t("s rep1", function() { return "" + rp.itemAt(1) })
        t("s created", function() { return "" + Qt.createQmlObject('import QtQuick; Item { property int w: 1 }', root) })
        t("s created2", function() { return "" + Qt.createQmlObject('import QtQuick; Item { }', root) })
        t("s createObject", function() { return "" + comp.createObject(root) })
        t("s createObject2", function() { return "" + comp2.createObject(root) })
        t("s list 1", function() { var o = lvc.createObject(root, { lv: root }); return "" + o.lv })
        t("s list 2", function() { var o = lvc.createObject(root, { lv: [root] }); return "" + o.lv })
        t("s list 3", function() { var o = lvc.createObject(root); o.lv = [root]; return "" + o.lv })
        t("s parent", function() { return "" + plain.parent })
        t("s toString", function() { return root.toString() })
        t("s String", function() { return String(plain) })
        t("s template", function() { return `${withProp}` })
        t("s concat", function() { return "x".concat(qo) })
        t("s join", function() { return [plain, qo].join("|") })
        t("s json", function() { return JSON.stringify({ o: qoPlain }) })
        t("s tostr call", function() { return Object.prototype.toString.call(root) })
        t("s renamed", function() { plain.objectName = "later"; return "" + plain })
        t("s unnamed", function() { plain.objectName = ""; return "" + plain })
        t("s anchors", function() { return "" + plain.anchors })
        t("s border", function() { return "" + plain.border })
        t("s null", function() { return "" + plain.anchors.fill })
        console.log("c1", root, plain, named)
        console.log("c2", [plain, named])
        console.log("c3", { o: qoPlain })
        console.log(root)
        console.info("c4", ld.item, tm)
        t("e1", function() { return plain.nope() })
        t("e2", function() { return named.nope() })
        t("e3", function() { return qo.v() })
        t("e4", function() { return tm.interval() })
        t("e5", function() { return plain.border.nope() })
        t("e6", function() { return new plain() })
        t("e7", function() { return plain() })
        t("e8", function() { return null in plain })
        t("e9", function() { return plain.nope.x })
        t("e10", function() { return ld.item.k() })
        t("e13", function() { return plain instanceof plain })
        t("e14", function() { return [].concat(plain).length })
        t("p1", function() { var r = Object.defineProperty(root, "a", { value: 3 }); return (r === root) + " " + root.a + " " + d(root, "a") })
        t("p2", function() { var r = Object.defineProperty(root, "width", { value: 3 }); return (r === root) + " " + root.width + " " + d(root, "width") })
        t("p3", function() { var r = Object.defineProperty(root, "fresh", { value: 3 }); return (r === root) + " " + root.fresh + " " + d(root, "fresh") + " " + ("fresh" in root) })
        t("p4", function() { var r = Object.defineProperty(root, "g", { get: function() { return 7 } }); return (r === root) + " " + root.g + " " + d(root, "g") })
        t("p5", function() { var r = Object.defineProperties(root, { m: { value: 1 }, a: { value: 9 } }); return (r === root) + " " + root.m + " " + root.a })
        t("p6", function() { return Reflect.defineProperty(root, "rf", { value: 1 }) + " " + root.rf })
        t("p7", function() { "use strict"; var r = Object.defineProperty(root, "st", { value: 3, writable: true, enumerable: true, configurable: true }); return (r === root) + " " + root.st })
        t("p8", function() { return typeof Object.defineProperty(qo, "v", { value: 5 }) + " " + qo.v })
        t("p9", function() { var was = plain.border.width; return typeof Object.defineProperty(plain.border, "width", { value: 5 }) + " " + (plain.border.width === was) + " " + typeof Object.defineProperty(plain.border, "fresh", { value: 5 }) + " " + plain.border.fresh })
        t("p10", function() { var was = txt.font.pixelSize; return typeof Object.defineProperty(txt.font, "pixelSize", { value: 5 }) + " " + (txt.font.pixelSize === was) + " " + typeof Object.defineProperty(txt.font, "fresh", { value: 5 }) + " " + txt.font.fresh })
        t("p11", function() { return typeof Object.defineProperty(root, Symbol.iterator, { value: 5 }) + " " + typeof root[Symbol.iterator] })
        t("p12", function() { return typeof Object.defineProperty(root, 0, { value: 5 }) + " " + root[0] })
        t("p13", function() { return typeof Object.defineProperty(root, "onAChanged", { value: 5 }) })
        t("p14", function() { return typeof Object.defineProperty(root, "t", { value: 5 }) + " " + typeof root.t })
        t("p15", function() { return typeof Object.defineProperty(root, "a") })
        t("p16", function() { return typeof Object.defineProperty(root, "a", 5) })
        t("p17", function() { return typeof Object.defineProperty(root, "a", { get: 5 }) })
        t("p18", function() { return typeof Object.defineProperty(root, "a", { get: function() {}, value: 1 }) })
        t("p19", function() { return Object.getOwnPropertyNames(root).length > 0 })
        t("p20", function() { return d(root, "a") + " | " + d(root, "width") + " | " + d(root, "t") + " | " + d(root, "none") + " | " + d(root, "aChanged") })
        t("p21", function() { return delete root.a })
        t("p22", function() { "use strict"; return delete root.a })
        t("p23", function() { return Object.isExtensible(root) + " " + Object.isFrozen(root) + " " + Object.isSealed(root) })
        var names = ["Locale", "Item", "Text", "Font"]
        for (var i = 0; i < names.length; i++) { let n = names[i]; t("w " + n, function() { var o = eval(n); return typeof o + " " + Object.prototype.toString.call(o) + " " + o + " ext=" + Object.isExtensible(o) + " frozen=" + Object.isFrozen(o) + " keys=" + Object.keys(o).length }) }
        t("l1", function() { return typeof Locale.LongFormat + " " + Locale.LongFormat + " " + Locale.ShortFormat + " " + Locale.NarrowFormat })
        t("l2", function() { return e(Locale, "LongFormat") + " | " + e(Locale, "none") + " | " + ("LongFormat" in Locale) })
        t("l3", function() { return Object.keys(Locale).join() })
        t("l4", function() { Locale.LongFormat = 5; return Locale.LongFormat })
        t("l5", function() { "use strict"; Locale.LongFormat = 5; return Locale.LongFormat })
        t("l6", function() { Locale.fresh = 5; return Locale.fresh })
        t("l7", function() { return Locale() })
        t("l8", function() { return new Locale() })
        t("l9", function() { return Locale.nope() })
        t("l10", function() { return typeof Locale.MetricSystem + " " + Locale.ImperialUSSystem + " " + Locale.CurrencyIsoCode + " " + Locale.OmitGroupSeparator })
        t("l11", function() { return Qt.locale() instanceof Locale })
        t("l12", function() { return "" + Object.getPrototypeOf(Locale) + " " + (Object.getPrototypeOf(Locale) === Object.prototype) })
        t("l13", function() { return String(Locale) + " " + JSON.stringify(Locale) })
        t("q1", function() { return ["LeftButton", "AlignLeft", "Key_A", "Horizontal", "rgba", "platform", "application", "inputMethod", "styleHints", "ElideRight", "nope"].map(function(k) { return k + ":" + e(Qt, k) }).join(" | ") })
        t("q2", function() { return Qt.propertyIsEnumerable("LeftButton") + " " + Qt.propertyIsEnumerable("rgba") + " " + Qt.hasOwnProperty("LeftButton") + " " + ("LeftButton" in Qt) })
        t("q3", function() { var n = 0, e = 0, f = 0; for (var k in Qt) { n++; if (typeof Qt[k] === "number") e++; if (typeof Qt[k] === "function") f++ } return n > 100 ? "many " + (e > 100) + " " + (f > 10) : n })
        t("q4", function() { var ks = Object.keys(Qt); return ks.indexOf("LeftButton") >= 0 && ks.indexOf("rgba") >= 0 })
        t("q8", function() { return e(Item, "TopLeft") + " | " + e(Text, "AlignLeft") + " | " + e(Easing, "Linear") + " | " + e(Font, "Bold") })
        t("q9", function() { return Object.keys(Item).indexOf("TopLeft") >= 0 })
        Qt.quit()
    }
}
