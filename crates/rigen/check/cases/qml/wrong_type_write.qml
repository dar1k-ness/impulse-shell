// A value of another type written to a property by a script and given by its binding: the
// property takes the value converted to its type, or keeps its value while the script's
// assignment throws (`!` and the message). The same for a property of a Qt type as for one
// the document declares, for a binding `createObject` or a script installs with
// `Qt.binding()`, and for a number written to an enumeration.
// qt: i number:12 !Cannot assign QString to int !Cannot assign QString to int number:2 number:-1 number:1 !Cannot assign std::nullptr_t to int !Cannot assign QJSValue to int !Cannot assign QJSValue to int !Cannot assign QDateTime to int !Cannot assign QObject* to int !Cannot assign QPointF to int | r number:12 number:NaN number:NaN number:2.7 number:-1 number:1 number:0 number:NaN number:NaN number:0 number:NaN number:NaN | b boolean:true boolean:true boolean:true boolean:true boolean:true boolean:true boolean:false boolean:true boolean:true boolean:true boolean:true boolean:true | s string:12 string:abc string:red string:2.7 string:-1 string:true string: !Cannot assign QJSValue to QString string: string:DATE !Cannot assign QObject* to QString !Cannot assign QPointF to QString | u object:12 object:abc object:red !Cannot assign double to QUrl !Cannot assign int to QUrl !Cannot assign bool to QUrl !Cannot assign std::nullptr_t to QUrl !Cannot assign QJSValue to QUrl !Cannot assign QJSValue to QUrl !Cannot assign QDateTime to QUrl !Cannot assign QObject* to QUrl !Cannot assign QPointF to QUrl | o !Cannot assign QString to QObject* !Cannot assign QString to QObject* !Cannot assign QString to QObject* !Cannot assign double to QObject* !Cannot assign int to QObject* !Cannot assign bool to QObject* null !Cannot assign QJSValue to QObject* !Cannot assign QJSValue to QObject* !Cannot assign QDateTime to QObject* item !Cannot assign QPointF to QObject* | li [number:49,number:50] [number:97,number:98,number:99] [number:114,number:101,number:100] [number:3] [number:-1] [number:1] [number:0] [number:0] [number:1,number:2] [number:0] [number:0] [number:0] | z number:12 number:NaN number:NaN number:2.7 number:-1 number:1 number:0 number:NaN number:NaN number:0 number:NaN number:NaN | visible boolean:true boolean:true boolean:true boolean:true boolean:true boolean:true boolean:false boolean:true boolean:true boolean:true boolean:true boolean:true | objectName string:12 string:abc string:red string:2.7 string:-1 string:true string: !Cannot assign QJSValue to QString string: string:DATE !Cannot assign QObject* to QString !Cannot assign QPointF to QString | opacity number:1 number:NaN number:NaN number:1 number:0 number:1 number:0 number:NaN number:NaN number:0 number:NaN number:NaN | width number:12 number:12 number:12 number:2.7 number:-1 number:1 number:0 number:0 number:0 number:0 number:0 number:0 | text string:12 string:abc string:red string:2.7 string:-1 string:true string: !Cannot assign QJSValue to QString string: string:DATE !Cannot assign QObject* to QString !Cannot assign QPointF to QString | horizontalAlignment !Cannot assign QString to QQuickText::HAlignment !Cannot assign QString to QQuickText::HAlignment !Cannot assign QString to QQuickText::HAlignment number:3 number:4294967295 number:1 !Cannot assign std::nullptr_t to QQuickText::HAlignment !Cannot assign QJSValue to QQuickText::HAlignment !Cannot assign QJSValue to QQuickText::HAlignment !Cannot assign QDateTime to QQuickText::HAlignment !Cannot assign QObject* to QQuickText::HAlignment !Cannot assign QPointF to QQuickText::HAlignment | maximumLineCount number:12 !Cannot assign QString to int !Cannot assign QString to int number:2 number:-1 number:1 !Cannot assign std::nullptr_t to int !Cannot assign QJSValue to int !Cannot assign QJSValue to int !Cannot assign QDateTime to int !Cannot assign QObject* to int !Cannot assign QPointF to int | radius number:12 number:NaN number:NaN number:2.7 number:-1 number:1 number:0 number:NaN number:NaN number:0 number:NaN number:NaN | color object:#000000 object:#000000 object:#ff0000 object:#000000 object:#000000 object:#000000 object:#000000 object:#000000 object:#000000 object:#000000 object:#000000 object:#000000 | interval number:12 !Cannot assign QString to int !Cannot assign QString to int number:2 number:-1 number:1 !Cannot assign std::nullptr_t to int !Cannot assign QJSValue to int !Cannot assign QJSValue to int !Cannot assign QDateTime to int !Cannot assign QObject* to int !Cannot assign QPointF to int | running boolean:true boolean:true boolean:true boolean:true boolean:true boolean:true boolean:false boolean:true boolean:true boolean:true boolean:true boolean:true | bound | number:12 number:12 boolean:true string:12 object:12 null [number:49,number:50] number:12 boolean:true string:12 number:1 number:12 string:12 number:1 number:12 number:12 object:#000000 number:12 boolean:true | number:12 number:NaN boolean:true string:abc object:abc null [number:97,number:98,number:99] number:NaN boolean:true string:abc number:NaN number:12 string:abc number:1 number:12 number:NaN object:#000000 number:12 boolean:true | number:12 number:NaN boolean:true string:red object:red null [number:114,number:101,number:100] number:NaN boolean:true string:red number:NaN number:12 string:red number:1 number:12 number:NaN object:#ff0000 number:12 boolean:true | number:2 number:2.7 boolean:true string:2.7 object:red null [number:3] number:2.7 boolean:true string:2.7 number:1 number:2.7 string:2.7 number:3 number:2 number:2.7 object:#000000 number:2 boolean:true | number:-1 number:-1 boolean:true string:-1 object:red null [number:-1] number:-1 boolean:true string:-1 number:0 number:-1 string:-1 number:4294967295 number:-1 number:-1 object:#000000 number:-1 boolean:true | number:1 number:1 boolean:true string:true object:red null [number:1] number:1 boolean:true string:true number:1 number:1 string:true number:1 number:1 number:1 object:#000000 number:1 boolean:true | number:1 number:0 boolean:false string: object:red null [number:0] number:0 boolean:false string:null number:0 number:0 string: number:1 number:1 number:0 object:#000000 number:1 boolean:false | number:1 number:NaN boolean:true string: object:red null [number:0] number:NaN boolean:true string:[object Object] number:NaN number:0 string: number:1 number:1 number:NaN object:#000000 number:1 boolean:true | number:1 number:NaN boolean:true string: object:red null [number:1,number:2] number:NaN boolean:true string:1,2 number:NaN number:0 string: number:1 number:1 number:NaN object:#000000 number:1 boolean:true | number:1 number:0 boolean:true string:DATE object:red null [number:0] number:0 boolean:true string:DATE number:0 number:0 string:DATE number:1 number:1 number:0 object:#000000 number:1 boolean:true | number:1 number:NaN boolean:true string:DATE object:red item [number:0] number:NaN boolean:true string:QQuickItem(PTR) number:NaN number:0 string:DATE number:1 number:1 number:NaN object:#000000 number:1 boolean:true | number:1 number:NaN boolean:true string:DATE object:red item [number:0] number:NaN boolean:true string:QPointF(1, 2) number:NaN number:0 string:DATE number:1 number:1 number:NaN object:#000000 number:1 boolean:true | Qt.binding | number:1 | number:1 | number:0 | string:5 | string: | enumeration | number:4294967295 number:0 number:2147483648 number:3 number:1 | number:4294967293 | createObject | number:4 number:1
import QtQuick

Item {
    id: root
    property string result: ""
    property var v: 1
    property Component bc: Component { Item { property int a: 1; property string t: "d" } }
    property Component ic: Component { Item { } }
    property Component tc: Component { Text { } }
    Item { id: other }
    QtObject {
        id: d
        property int i: 1; property real r: 1.5; property bool b: true; property string s: "a"
        property url u: "u"; property QtObject o: null; property list<int> li: [1]
    }
    Item { id: it }
    Text { id: tx }
    Rectangle { id: rc }
    Timer { id: tm }
    QtObject {
        id: bd
        property int i: root.v; property real r: root.v; property bool b: root.v
        property string s: root.v; property url u: root.v; property QtObject o: root.v
        property list<int> li: root.v
    }
    Item { id: bit; z: root.v; visible: root.v; objectName: root.v; opacity: root.v; width: root.v }
    Text { id: btx; text: root.v; horizontalAlignment: root.v; maximumLineCount: root.v }
    Rectangle { id: brc; radius: root.v; color: root.v }
    Timer { id: btm; interval: root.v; running: root.v }

    function show(v) {
        if (v === other) return "item"
        if (v === null) return "null"
        if (v instanceof Date) return "date:" + v.getTime()
        if (typeof v === "object" && typeof v.length === "number") return "[" + Array.from(v).map(show) + "]"
        // An address and a date's text (the time zone's) are not compared here.
        return typeof v + ":" + String(v).replace(/0x[0-9a-f]+/, "PTR").replace(/^Thu Jan 1 .* 1970 GMT.*$/, "DATE")
    }
    function wr(o, k, v) {
        try { o[k] = v } catch (e) { return "!" + e.message }
        return show(o[k])
    }
    function made(c, k, f) {
        const o = c.createObject(root, {[k]: Qt.binding(f)})
        const r = show(o[k])
        o.destroy()
        return r
    }
    Component.onCompleted: {
        const values = ["12", "abc", "red", 2.7, -1, true, null, ({}), [1, 2], new Date(0), other, Qt.point(1, 2)]
        const written = [[d, "i"], [d, "r"], [d, "b"], [d, "s"], [d, "u"], [d, "o"], [d, "li"],
            [it, "z"], [it, "visible"], [it, "objectName"], [it, "opacity"], [it, "width"],
            [tx, "text"], [tx, "horizontalAlignment"], [tx, "maximumLineCount"],
            [rc, "radius"], [rc, "color"], [tm, "interval"], [tm, "running"]]
        const bound = [[bd, "i"], [bd, "r"], [bd, "b"], [bd, "s"], [bd, "u"], [bd, "o"], [bd, "li"],
            [bit, "z"], [bit, "visible"], [bit, "objectName"], [bit, "opacity"], [bit, "width"],
            [btx, "text"], [btx, "horizontalAlignment"], [btx, "maximumLineCount"],
            [brc, "radius"], [brc, "color"], [btm, "interval"], [btm, "running"]]
        const out = []
        for (const [o, k] of written)
            out.push(k + " " + values.map(v => wr(o, k, v)).join(" "))
        out.push("bound")
        for (const v of values) {
            root.v = v
            out.push(bound.map(([o, k]) => show(o[k])).join(" "))
        }
        out.push("Qt.binding")
        out.push(made(root.bc, "a", function() { return "x" }))
        const o = root.bc.createObject(root)
        o.a = Qt.binding(function() { return "x" })
        out.push(show(o.a))
        out.push(made(root.ic, "width", function() { return "x" }))
        out.push(made(root.tc, "text", function() { return 5 }))
        out.push(made(root.tc, "text", function() { return undefined }))
        out.push("enumeration")
        const t = root.tc.createObject(root)
        out.push([-1, 4294967296, -2147483648, 2.5, true].map(v => wr(t, "horizontalAlignment", v)).join(" "))
        out.push(wr(t, "wrapMode", -3))
        out.push("createObject")
        const made2 = root.ic.createObject(root, {z: "4", opacity: true})
        out.push(show(made2.z) + " " + show(made2.opacity))
        result = out.join(" | ")
    }
}
