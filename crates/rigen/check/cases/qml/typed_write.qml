// A write from JavaScript into a typed property converts the value as Qt does, and throws
// where Qt has no conversion, the property keeping its value; so does a write to a member
// that is not a property (a method, a signal, a handler, an unknown name).
// qt: n=number 2;n=number 12;n=number 4;n=Error: Cannot assign QString to int (4);n=number 1;n=Error: Cannot assign std::nullptr_t to int (1);n=Error: Cannot assign [undefined] to int (1);n=Error: Cannot assign QJSValue to int (1);n=number 1410065408;r=number 2.5;r=number NaN;r=number 1;r=number 0;r=Error: Cannot assign [undefined] to double (0);r=number 7;b=boolean false;b=boolean false;b=boolean true;b=boolean false;b=Error: Cannot assign [undefined] to bool (false);b=boolean true;s=string 5;s=string 1.5;s=string 1e+21;s=string true;s=string ;s=Error: Cannot assign [undefined] to QString ();s=Error: Cannot assign QJSValue to QString ();s=string ab;s=string u/v;s=Error: Cannot assign QPointF to QString (u/v);u=object c/d;u=Error: Cannot assign int to QUrl (c/d);u=Error: Cannot assign std::nullptr_t to QUrl (c/d);u=Error: Cannot assign [undefined] to QUrl (c/d);u=object e/f;c=object #0000ff;c=Error: Cannot assign [undefined] to QColor (#0000ff);it=object <sub>;it=object null;it=Error: Cannot assign int to QQuickItem* (null);it=Error: Cannot assign QString to QQuickItem* (null);it=Error: Cannot assign [undefined] to QQuickItem* (null);tm=object <timer>;tm=Error: Cannot assign QObject* to QQmlTimer* (<timer>);li=Error: Cannot assign [undefined] to QList<int> (1);li=Error: Cannot assign JavaScript function to QList<int> (1);v=undefined undefined;v=string x;inner=number 9;inner=Error: Cannot assign QString to int (9);ro=TypeError: Cannot assign to read-only property "ro" (4);nope=Error: Cannot assign to non-existent property "nope" (undefined);m=TypeError: Cannot assign to read-only property "m" (function);sig=TypeError: Cannot assign to read-only property "sig" (function);TypeError: Cannot assign to read-only property "sig";TypeError: Cannot assign to read-only property "nChanged";TypeError: Cannot assign to read-only property "nChanged";width ok;objectName ok;Error: Cannot assign [undefined] to bool;Error: Cannot assign to non-existent property "destroy";TypeError: Cannot assign to read-only property "start";TypeError: Cannot assign to read-only property "triggered";Error: Cannot assign QString to int;Error: Cannot assign to non-existent property "nope";Error: Cannot assign [undefined] to double;,0,x/y;<sub>,2,p/q;<sub>,2,p/q
import QtQuick
Item {
    id: root
    property string result: ""
    property int n: 1
    property real r: 1.5
    property bool b: true
    property string s: "s"
    property url u: "a/b"
    property color c: "red"
    property Item it
    property Timer tm
    property list<int> li: [1]
    property var v
    property alias inner: sub.n2
    readonly property int ro: 4
    property var src: ({})
    property Item bound: src.item
    property list<int> boundList: src.list
    property url boundUrl: src.url ?? "x/y"
    signal sig(int a)
    function m() {}
    Item {
        id: sub
        objectName: "sub"
        property int n2: 2
    }
    Timer {
        id: timer
        objectName: "timer"
    }
    function str(x) {
        if (typeof x === "function")
            return "function"
        return x && x.objectName ? "<" + x.objectName + ">" : x
    }
    function w(name, val) {
        let out
        try {
            root[name] = val
            out = typeof root[name] + " " + str(root[name])
        } catch (e) {
            out = e.name + ": " + e.message + " (" + str(root[name]) + ")"
        }
        result += name + "=" + out + ";"
    }
    function on(o, name, val) {
        try {
            o[name] = val
            result += name + " ok;"
        } catch (e) {
            result += e.name + ": " + e.message + ";"
        }
    }
    Component.onCompleted: {
        for (const x of [2.7, "12", " 3.5 ", "abc", true, null, undefined, {}, 1e10])
            w("n", x)
        for (const x of ["2.5", "abc", true, null, undefined, [7]])
            w("r", x)
        for (const x of [0, "", "x", null, undefined, {}])
            w("b", x)
        for (const x of [5, 1.5, 1e21, true, null, undefined, {}, ["a", "b"], Qt.url("u/v"), Qt.point(1, 2)])
            w("s", x)
        for (const x of ["c/d", 5, null, undefined, Qt.url("e/f")])
            w("u", x)
        for (const x of ["blue", undefined])
            w("c", x)
        for (const x of [sub, null, 5, "x", undefined])
            w("it", x)
        for (const x of [timer, sub])
            w("tm", x)
        for (const x of [undefined, function () {}])
            w("li", x)
        for (const x of [undefined, "x"])
            w("v", x)
        for (const x of ["9", "zz"])
            w("inner", x)
        w("ro", 5)
        w("nope", 3)
        w("m", 3)
        w("sig", 3)
        on(root, "onSig", 3)
        on(root, "nChanged", 3)
        on(root, "onNChanged", 3)
        on(root, "width", "x")
        on(root, "objectName", 5)
        on(root, "visible", undefined)
        on(root, "destroy", 3)
        on(timer, "start", 3)
        on(timer, "onTriggered", 3)
        on(timer, "interval", "x")
        on(sub, "nope", 1)
        on(root.anchors, "margins", undefined)
        result += [str(bound), boundList.length, boundUrl].join(",") + ";"
        src = { item: sub, list: [1, 2], url: "p/q" }
        result += [str(bound), boundList.length, boundUrl].join(",") + ";"
        src = { url: 5 }
        result += [str(bound), boundList.length, boundUrl].join(",")
    }
}
