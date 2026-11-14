import QtQuick
import QtQuick.Controls
import QtQuick.Templates as T
import QtQuick.Layouts
Item {
    id: root
    objectName: "rt"
    component Ic: Rectangle { property int q: 1 }
    component Bare: Item { }
    Item { id: plain }
    Item { id: named; objectName: "nm" }
    Item { id: own; property int z1: 1 }
    Item { id: own2; property int z1: 1 }
    Rectangle { id: sig; signal foo() }
    Text { id: fn; function f() {} }
    QtObject { id: qo }
    QtObject { id: qo2; property int k: 0 }
    Button { id: bt }
    Button { id: bt2; objectName: "b2" }
    Button { id: bt3; property int m: 0 }
    Ic { id: ic }
    Ic { id: ic2; property int w2: 0 }
    Bare { id: bare }
    Timer { id: tm }
    ListModel { id: lm }
    Component { id: cp; Item {} }
    Component { id: cp2; Item { property int inComponent: 1 } }
    Repeater { id: rp; model: 2; Item { objectName: "dl" } }
    Repeater { id: rp2; model: 2; Item { property int d: index } }
    ButtonGroup { id: bg }
    Action { id: ac }
    Control { id: ct }
    Label { id: lb }
    Popup { id: pp }
    T.Button { id: tb }
    T.Label { id: tl }
    TextField { id: tf }
    ScrollView { id: sv }
    StackView { id: st }
    AbstractButton { id: ab }
    Container { id: cn }
    RowLayout { id: rl }
    ListView { id: lv }
    Rectangle { id: on1; Behavior on x { NumberAnimation {} } }
    Rectangle { id: on2; NumberAnimation on y { to: 1 } }
    Item { id: en; enum E { A, B } }
    Item { id: al; property alias q: lb.text }
    Item { id: rq; required property int need; need: 1 }
    Item { id: hd; onXChanged: {} }
    Item { id: dflt; default property list<QtObject> stuff }
    Item { id: prop; property string toString: "text" }
    F21Plain { id: fp }
    F21Plain { id: fp2; objectName: "second" }
    F21Plain { id: fp3; property int more: 1 }
    F21Own { id: fo }
    F21Derived { id: fd }
    Loader { id: ld; source: "f21lower.qml" }
    Loader { id: ld2; source: "f21lowerown.qml" }
    Loader { id: ld3; source: "F21Plain.qml" }
    Loader { id: ld4; sourceComponent: cp2 }
    Component.onCompleted: {
        const norm = (v) => String(v).replace(/0x[0-9a-f]+/g, "0x").replace(/_(QMLTYPE|QML)_\d+/g, "_$1_N")
        const cls = (v) => String(v).split("(")[0]
        console.log("root", norm(root))
        console.log("plain", norm(plain), norm(plain.toString()), norm("" + plain), norm(`${plain}`))
        console.log("named", norm(named))
        console.log("own", norm(own), cls(own) === cls(own2), cls(own) === cls(own))
        console.log("sig", norm(sig))
        console.log("fn", norm(fn))
        console.log("qo", norm(qo), norm(qo2))
        console.log("bt", norm(bt), norm(bt2), norm(bt3), cls(bt) === cls(bt2), cls(bt) === cls(bt3))
        console.log("ic", norm(ic), norm(ic2), norm(bare))
        console.log("tm", norm(tm), norm(lm), norm(cp), norm(rp), norm(rp.itemAt(0)))
        console.log("rp", norm(rp2.itemAt(0)), cls(rp2.itemAt(0)) === cls(rp2.itemAt(1)), rp2.itemAt(0) == rp2.itemAt(1))
        console.log("cp", norm(cp.createObject(root)), norm(cp2.createObject(root)), cls(cp2.createObject(root)) === cls(ld4.item))
        console.log("anchors", norm(root.parent), norm(root.anchors), norm(lv.contentItem))
        console.log("controls", norm(bg), norm(ac), norm(ct), norm(lb), norm(pp))
        console.log("templates", norm(tb), norm(tl), norm(tf), norm(sv), norm(st))
        console.log("more", norm(ab), norm(cn), norm(rl), norm(lv))
        console.log("members", norm(on1), norm(on2), norm(en), norm(al), norm(rq), norm(hd), norm(dflt))
        console.log("toString", norm(prop), norm(prop.toString()), norm("" + prop))
        console.log("file", norm(fp), norm(fp2), norm(fp3), cls(fp) === cls(fp2), cls(fp) === cls(fp3))
        console.log("file own", norm(fo), fo.inside(), norm(fd), fd.inside(), cls(fo) === cls(fd))
        console.log("loader", norm(ld.item), norm(ld2.item), norm(ld3.item), cls(ld3.item) === cls(fp), norm(ld4.item))
        const made = Qt.createComponent("F21Own.qml").createObject(root)
        const text = Qt.createQmlObject("import QtQuick; Item { property int t: 1 }", root)
        const text2 = Qt.createQmlObject("import QtQuick; Item { }", root)
        console.log("made", norm(made), cls(made) === cls(fo), norm(text), norm(text2))
        console.log("json", JSON.stringify(qo2), JSON.stringify({a: qo}))
        console.log("is", plain == plain.toString(), plain === plain.toString(), typeof plain, Object.prototype.toString.call(plain))
        console.log("eq", lb == String(lb), String(lb) == lb, lb + "" === lb.toString(), [lb].join() === String(lb), lb != String(tl))
        const k = {}; k[lb] = 1; const m = {}; m[String(lb)] = 2
        console.log("key", norm(Object.keys(k)[0]), k[lb], lb in k, m[lb], Object.keys(k)[0] === String(lb))
        console.log("ops", lb + 1 === String(lb) + "1", [lb, lb].join("|") === String(lb) + "|" + lb.toString(), isNaN(lb), !!lb)
        console.log("concat", String([lb]) === String(lb), "x".concat(lb) === "x" + lb, typeof lb.toString, lb.toString === lb.toString)
        console.log("array", norm([plain, qo, named]), norm(String([bt2, null, undefined, 1])))
    }
}
