// A QML object in JS is an object: it has Object.prototype's methods, `instanceof` takes QML
// types and Object, `in` sees its methods, signals and signal handlers, and `==` against a
// string compares its string form.
// qt: [true,true,true,true,false] [true,true,true,true,true,false,true,true] [true,true,true,true,true,true,false,false,false] [true,true,false,true,true,true] ["function","function","function","function","function","function","object","function","function"] [true,true,true,true,false,true,true] [true,false,true,true] [true,false,true,true] [5,true,false] [true,true,false,true,true,true,false] [true,true,true,true,false,false,true,false] [false,false,false,false,false] [false,false,false,true,true] THROWS TypeError THROWS TypeError [true,true,true,false,false,false,false,false,true,false,false] [true,true,false,false,true,true,true] [true,true,true,false,false,false,true] [true,true,true,true,true,true,true,true,false] [true,true,true,true,true,true,true,true,true,true,true] [true,true,true,true,true,true,true,true,true,false,false,false,false,false,false] [true,true,true,true,true,false,false,false] [true,false,false,true,true,true]
import QtQuick
import QtQml
Item {
    id: root
    property string result: ""
    property int own: 1
    property var jsobj: ({ k: 1 })
    signal sig()
    function meth() {}
    QtObject { id: qo; property int a: 1 }
    EvSub { id: sub }
    Timer { id: tm }
    function t(fn) {
        var r
        try { r = JSON.stringify(fn()) } catch (e) { r = "THROWS " + e.name }
        result += (result ? " " : "") + r
    }
    function run() {
        t(() => [root.hasOwnProperty("objectName"), qo.hasOwnProperty("a"), root.hasOwnProperty("own"), root.hasOwnProperty("width"), root.hasOwnProperty("nope")])
        t(() => [root.hasOwnProperty("sig"), root.hasOwnProperty("onSig"), root.hasOwnProperty("meth"), root.hasOwnProperty("destroy"), root.hasOwnProperty("toString"), root.hasOwnProperty("hasOwnProperty"), root.hasOwnProperty("ownChanged"), root.hasOwnProperty("onOwnChanged")])
        t(() => [sub.hasOwnProperty("subProp"), sub.hasOwnProperty("color"), sub.hasOwnProperty("subSig"), sub.hasOwnProperty("x"), sub.hasOwnProperty("parent"), sub.hasOwnProperty("anchors"), sub.hasOwnProperty("qo"), root.hasOwnProperty("qo"), root.hasOwnProperty("root")])
        t(() => [qo.propertyIsEnumerable("a"), qo.propertyIsEnumerable("objectName"), qo.propertyIsEnumerable("nope"), root.propertyIsEnumerable("sig"), root.propertyIsEnumerable("meth"), root.propertyIsEnumerable("destroy")])
        t(() => [typeof qo.valueOf, typeof qo.hasOwnProperty, typeof qo.propertyIsEnumerable, typeof qo.isPrototypeOf, typeof qo.toLocaleString, typeof qo.constructor, typeof qo.__proto__, typeof qo.toString, typeof qo.destroy])
        t(() => [qo.valueOf() === qo, qo.hasOwnProperty === Object.prototype.hasOwnProperty, qo.valueOf === Object.prototype.valueOf, qo.constructor === Object, qo.isPrototypeOf(qo), Object.prototype.isPrototypeOf(qo), qo.toLocaleString() === qo.toString()])
        t(() => [Object.prototype.hasOwnProperty.call(qo, "a"), Object.prototype.hasOwnProperty.call(qo, "b"), Object.prototype.propertyIsEnumerable.call(qo, "a"), Object.prototype.valueOf.call(qo) === qo])
        t(() => [Object.getPrototypeOf(qo) === Object.prototype, Object.getPrototypeOf(Object.getPrototypeOf(qo)) === Object.prototype, Object.getPrototypeOf(Object.getPrototypeOf(qo)) === null, Object.getPrototypeOf(qo) === Object.getPrototypeOf(root)])
        t(() => { Object.prototype.evProbe = 5; var r = [qo.evProbe, "evProbe" in qo, qo.hasOwnProperty("evProbe")]; delete Object.prototype.evProbe; return r })
        t(() => [root instanceof Item, root instanceof QtObject, root instanceof Rectangle, root instanceof Object, qo instanceof Object, qo instanceof QtObject, qo instanceof Item])
        t(() => [sub instanceof EvSub, sub instanceof Rectangle, sub instanceof Item, sub instanceof QtObject, sub instanceof Text, root instanceof EvSub, tm instanceof Timer, tm instanceof Item])
        t(() => [({}) instanceof Item, null instanceof Item, 5 instanceof QtObject, jsobj instanceof QtObject, [] instanceof Item])
        t(() => [root instanceof Function, root instanceof Array, root instanceof Error, Qt.point(1,2) instanceof Object, Qt.rgba(1,0,0,1) instanceof Object])
        t(() => root instanceof qo)
        t(() => root instanceof Item.TopLeft)
        t(() => [root == String(root), String(root) == root, root == root.toString(), root != String(root), root == "x", root == 1, root == null, root == undefined, root == root, qo == root, root === String(root)])
        t(() => [Qt.rgba(1,0,0,1) == "#ff0000", "#ff0000" == Qt.rgba(1,0,0,1), Qt.rgba(1,0,0,1) != "#ff0000", Qt.rgba(1,0,0,1) == "red", Qt.rgba(1,0,0,1) == Qt.rgba(1,0,0,1), Qt.point(1,2) == "QPointF(1, 2)", Qt.point(1,2) == String(Qt.point(1,2))])
        t(() => [Math.max == String(Math.max), String(Math.max) == Math.max, Math.max == Math.max, root == true, root == false, qo == 0, (qo + "") == qo])
        t(() => ["destroy" in root, "toString" in root, "onSig" in root, "sig" in root, "meth" in root, "own" in root, "ownChanged" in root, "onOwnChanged" in root, "nope" in root])
        t(() => ["hasOwnProperty" in root, "valueOf" in root, "constructor" in root, "__proto__" in root, "objectName" in root, "objectNameChanged" in root, "onObjectNameChanged" in root, "width" in root, "onWidthChanged" in root, "children" in root, "data" in root])
        t(() => ["subProp" in sub, "subSig" in sub, "onSubSig" in sub, "color" in sub, "onColorChanged" in sub, "a" in qo, "onAChanged" in qo, "aChanged" in qo, "destroy" in qo, "qo" in root, "root" in root, "sub" in qo, "onNope" in root, "onDestroy" in root, "onMeth" in root])
        t(() => ["triggered" in tm, "onTriggered" in tm, "start" in tm, "running" in tm, "onRunningChanged" in tm, "onStart" in tm, 0 in root, Symbol.iterator in root])
        t(() => ["connect" in root.sig, "onCompleted" in root, "Component" in root, "anchors" in root, "fill" in root.anchors, "parent" in root])
    }
    Component.onCompleted: run()
}
