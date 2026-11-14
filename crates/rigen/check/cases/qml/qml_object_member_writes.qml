// A write from script to a member of a QML object, as in Qt 6.12 (QV4's
// QObjectWrapper::virtualPut): a method, a signal, a signal's handler and a `…Changed` are
// read-only; an object a QML document created takes no new name, but keeps an array index;
// a group object (`anchors`) keeps a new name as a JavaScript property of its own, which
// `delete` removes; a value type (`font`) drops it.
// qt: new Error: Cannot assign to non-existent property "foo";new this Error: Cannot assign to non-existent property "foo2";new compound Error: Cannot assign to non-existent property "foo";new increment Error: Cannot assign to non-existent property "foo";new plain Error: Cannot assign to non-existent property "foo";new child Error: Cannot assign to non-existent property "foo";new created Error: Cannot assign to non-existent property "foo";new createQmlObject Error: Cannot assign to non-existent property "foo";new component Error: Cannot assign to non-existent property "foo";new parent Error: Cannot assign to non-existent property "foo";method TypeError: Cannot assign to read-only property "ping";method function TypeError: Cannot assign to read-only property "ping";signal TypeError: Cannot assign to read-only property "poked";handler TypeError: Cannot assign to read-only property "poked";changed TypeError: Cannot assign to read-only property "declaredChanged";cpp method TypeError: Cannot assign to read-only property "forceActiveFocus";cpp changed TypeError: Cannot assign to read-only property "widthChanged";cpp handler TypeError: Cannot assign to read-only property "widthChanged";component method TypeError: Cannot assign to read-only property "createObject";toString Error: Cannot assign to non-existent property "toString";destroy Error: Cannot assign to non-existent property "destroy";hasOwnProperty Error: Cannot assign to non-existent property "hasOwnProperty";__proto__ Error: Cannot assign to non-existent property "__proto__";read-only TypeError: Cannot assign to read-only property "childrenRect";property 8,5;index 1,true,true;index string 2,2,true;index float Error: Cannot assign to non-existent property "1.5";index negative Error: Cannot assign to non-existent property "-1";index delete true,,false,1;index other ,;index child a;group 1,true,true,true;group other undefined;group delete true,,false;group function true;group compound 4;group object 2;group index a;group keys zz,aa;group for in 2;group descriptor {"value":1,"writable":true,"enumerable":true,"configurable":true};group member true,3;group changed TypeError: Cannot assign to read-only property "leftMarginChanged";group destroy function;border 1;text font undefined;color undefined;held object 1;Math 1;keys -1;
import QtQuick
Rectangle {
    id: root
    property int declared: 7
    property var held
    property string result: ""
    signal poked()
    function ping() { return "pong" }
    Item { id: inner; property int innerProp: 1 }
    QtObject { id: plain }
    Component { id: comp; Item { property int own: 1 } }
    Text { id: txt }
    function t(what, f) {
        var r
        try { r = String(f()) } catch (e) { r = e.name + ": " + e.message }
        result += what + " " + r + ";"
    }
    function run() {
        t("new", () => { root.foo = 1; return root.foo })
        t("new this", () => { this.foo2 = 1; return this.foo2 })
        t("new compound", () => { root.foo += 1; return root.foo })
        t("new increment", () => { root.foo++; return root.foo })
        t("new plain", () => { plain.foo = 1; return plain.foo })
        t("new child", () => { root.children[0].foo = 1; return root.children[0].foo })
        t("new created", () => { var o = comp.createObject(root); o.foo = 1; return o.foo })
        t("new createQmlObject", () => { var o = Qt.createQmlObject("import QtQuick; Item {}", root); o.foo = 1; return o.foo })
        t("new component", () => { comp.foo = 1; return comp.foo })
        t("new parent", () => { inner.parent.foo = 1; return inner.parent.foo })
        t("method", () => { root.ping = 1; return typeof root.ping })
        t("method function", () => { root.ping = function() { return "mine" }; return root.ping() })
        t("signal", () => { root.poked = 1; return typeof root.poked })
        t("handler", () => { root.onPoked = function() {}; return typeof root.onPoked })
        t("changed", () => { root.declaredChanged = 1; return typeof root.declaredChanged })
        t("cpp method", () => { root.forceActiveFocus = 1; return typeof root.forceActiveFocus })
        t("cpp changed", () => { root.widthChanged = 1; return 0 })
        t("cpp handler", () => { root.onWidthChanged = 1; return 0 })
        t("component method", () => { comp.createObject = 1; return 0 })
        t("toString", () => { root.toString = 1; return typeof root.toString })
        t("destroy", () => { root.destroy = 1; return typeof root.destroy })
        t("hasOwnProperty", () => { root.hasOwnProperty = 1; return typeof root.hasOwnProperty })
        t("__proto__", () => { root.__proto__ = null; return typeof root.toString })
        t("read-only", () => { root.childrenRect = 1; return 0 })
        t("property", () => { root.declared = 8; root.width = 5; return [root.declared, root.width] })
        t("index", () => { root[0] = 1; return [root[0], 0 in root, root.hasOwnProperty(0)] })
        t("index string", () => { root["1"] = 2; return [root[1], root["1"], "1" in root] })
        t("index float", () => { root[1.5] = 2; return root[1.5] })
        t("index negative", () => { root[-1] = 2; return root[-1] })
        t("index delete", () => [delete root[1], root[1], 1 in root, root[0]])
        t("index other", () => [inner[0], plain[0]])
        t("index child", () => { inner[0] = "a"; return inner[0] })
        t("group", () => { root.anchors.foo = 1; return [root.anchors.foo, "foo" in root.anchors, root.anchors.hasOwnProperty("foo"), Object.keys(root.anchors).indexOf("foo") >= 0] })
        t("group other", () => inner.anchors.foo)
        t("group delete", () => [delete root.anchors.foo, root.anchors.foo, "foo" in root.anchors])
        t("group function", () => { root.anchors.f = function() { return this === root.anchors }; return root.anchors.f() })
        t("group compound", () => { root.anchors.n = 1; root.anchors.n++; root.anchors.n += 2; return root.anchors.n })
        t("group object", () => { root.anchors.o = {a: 1}; root.anchors.o.a++; return root.anchors.o.a })
        t("group index", () => { root.anchors[0] = "a"; return root.anchors[0] })
        t("group keys", () => { root.anchors.zz = 1; root.anchors.aa = 2; var k = Object.keys(root.anchors); return k.slice(k.length - 2) })
        t("group for in", () => { var n = 0; for (var k in root.anchors) if (k === "zz" || k === "aa") n++; return n })
        t("group descriptor", () => JSON.stringify(Object.getOwnPropertyDescriptor(root.anchors, "zz")))
        t("group member", () => { root.anchors.margins = 3; return [delete root.anchors.margins, root.anchors.margins] })
        t("group changed", () => { root.anchors.leftMarginChanged = 1; return 0 })
        t("group destroy", () => { root.anchors.destroy = 1; return typeof root.anchors.destroy })
        t("border", () => { root.border.foo = 1; return root.border.foo })
        t("text font", () => { txt.font.foo = 1; return txt.font.foo })
        t("color", () => { root.color.foo = 1; return root.color.foo })
        t("held object", () => { root.held = {}; root.held.foo = 1; return root.held.foo })
        t("Math", () => { Math.foo = 1; return Math.foo })
        t("keys", () => Object.keys(root).indexOf("foo"))
    }
    Component.onCompleted: run()
}
