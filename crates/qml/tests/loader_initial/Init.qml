import QtQuick
// Every value written to every property three ways: by a script, by `Loader.setSource`,
// and the row says what the property reads after each.
Item {
    id: root
    property string report: ""
    signal finished()
    QtObject { id: qo; objectName: "qo" }
    Item { id: anItem; objectName: "anItem" }
    Component { id: typed; Typed {} }
    Component { id: ld; Loader {} }
    property var values: ({
        "1": 1, "1.5": 1.5, "'x'": "x", "''": "", "true": true, "null": null, "undefined": undefined,
        "{}": ({}), "[]": [], "[1,2]": [1, 2], "'red'": "red", "'1,2'": "1,2", "url": Qt.resolvedUrl("a.qml"),
        "QtObject": qo, "Item": anItem, "Qt.point": Qt.point(1, 2), "Qt.rect": Qt.rect(1, 2, 3, 4), "Qt.size": Qt.size(1, 2),
        "Date": new Date(0), "'2020-01-02'": "2020-01-02", "fn": function() { return 3 }, "NaN": NaN, "'7'": "7",
        "Qt.rgba": Qt.rgba(1, 0, 0, 1), "Qt.font": Qt.font({ pixelSize: 9 }), "1e20": 1e20, "-1": -1
    })
    property var names: Object.keys(values)
    property var props: ["u", "s", "i", "r", "b", "c", "p", "d", "f", "v", "o", "it", "li", "rc", "sz", "ro", "bound", "al", "width", "nothing", "parent", "objectName", "anchors.margins", "onWidthChanged"]
    readonly property string family: "\"family\":" + JSON.stringify(Qt.application.font.family)
    function show(v) {
        if (v === null) return "null"
        if (v === undefined) return "undefined"
        if (typeof v === "object" && v.objectName !== undefined) return "obj:" + v.objectName
        if (typeof v === "object" && v instanceof Date) return "date:" + (isNaN(v) ? "invalid" : v.toISOString())
        if (typeof v === "function") return "function"
        // The application font's family is the platform's, not something a write decides.
        if (typeof v === "object") { try { return "object:" + JSON.stringify(v).split(family).join("\"family\":the default") } catch (e) { return "object:" + v } }
        return typeof v + ":" + v
    }
    // `a.b.c` names `c` of `a.b`.
    function owner(o, k) { return k.split(".").slice(0, -1).reduce((at, part) => at[part], o) }
    function last(k) { return k.split(".").pop() }
    function count() { return props.length * names.length }
    function go(n) {
        var k = props[Math.floor(n / names.length)], name = names[n % names.length], val = values[name]
        var init = {}; init[k] = val
        var t = typed.createObject(root), wrote
        try { owner(t, k)[last(k)] = val; wrote = show(owner(t, k)[last(k)]) } catch (e) { wrote = "throws " + e }
        t.destroy()
        var l = ld.createObject(root), set
        try { l.setSource("Typed.qml", init); set = l.status + " " + (l.item ? show(owner(l.item, k)[last(k)]) : "no item") } catch (e) { set = "throws " + e }
        l.destroy()
        return k + " = " + name + " | script " + wrote + " | setSource " + set
    }
    function all() {
        var out = []
        for (var n = 0; n < count(); n++) { out.push(go(n)); console.log("MARK") }
        report = out.join("\n")
        Qt.callLater(finished)
    }
}
