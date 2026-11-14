// The code of a `.js` file that is no `.pragma library` runs in a context below the importing
// document's (Qt 6.12, QQmlScriptData::qmlContextDataForContext): it reads and writes the
// document's ids and its root object's members behind its own names, per importing instance,
// and sees none of the document's types and import qualifiers. A `.pragma library` and the
// files it imports see no document.
// qt: read prop 7;read cpp 10;read id object,true;read inner 1;read method pong;read missing undefined;read missing throw ReferenceError: nowhere is not defined;write prop 21,21;write cpp 33,33;write compound 23,23;write incr 24,24;write id Error: left-hand side of assignment operator is not an lvalue;write method TypeError: Cannot assign to read-only property "ping";write signal TypeError: Cannot assign to read-only property "poked";write missing Error: Invalid write to global property "nowhere2";write own 5,5,qml;shadow 101,24;emit 1;this object,false;nested 25;via arg 24;call other 25;stored fn 24;fn value 24;call 24;child 24,1,undefined;child this object,false;own first number,5;shadowed string,js;parent prop undefined;caller prop ReferenceError: onlyChild is not defined;other read 500;other own 4;other write own 9,9;other write 21,24;other var 5,5,5;other shadowed string,js;bound 48;bound after 100;inner read number,object,string;inner write 77,77;included 77,object;map 78|79;typeofs undefined|object|undefined|object|object|string|object;type name ReferenceError: Rectangle is not defined;delete true,77;ns read undefined,false,false,undefined,undefined;ns keys 42;instances 41,42,41,42,5;instances scope 21,600;same fn false,true;lib read undefined;lib id undefined;lib write Error: Invalid write to global property "declared";
import QtQuick
import "js_scope_plain.js" as Plain
import "js_scope_lib.js" as Lib
Rectangle {
    id: root
    width: 10
    property int declared: 7
    property int count: 0
    property string own: "qml"
    property string result: ""
    signal poked()
    onPoked: count++
    function ping() { return "pong" }
    Item { id: inner; property int innerProp: 1; property int childOnly: 2
        function viaChild() { return Plain.readProp() + "," + Plain.readInner() + "," + Plain.parentProp() }
        function childThis() { return Plain.thisIs() }
    }
    JsScopeOther { id: other }
    JsScopeOther { id: other2 }
    property var bound: Plain.readProp() * 2
    property var fnValue: Plain.readProp
    function t(what, f) {
        var r
        try { r = String(f()) } catch (e) { r = e.name + ": " + e.message }
        result += what + " " + r + ";"
    }
    Component.onCompleted: {
        t("read prop", () => Plain.readProp())
        t("read cpp", () => Plain.readCpp())
        t("read id", () => Plain.readId())
        t("read inner", () => Plain.readInner())
        t("read method", () => Plain.readMethod())
        t("read missing", () => Plain.readMissing())
        t("read missing throw", () => Plain.readMissingThrow())
        t("write prop", () => Plain.writeProp() + "," + root.declared)
        t("write cpp", () => Plain.writeCpp() + "," + root.width)
        t("write compound", () => Plain.writeCompound() + "," + root.declared)
        t("write incr", () => Plain.writeIncr() + "," + root.declared)
        t("write id", () => Plain.writeId())
        t("write method", () => Plain.writeMethod())
        t("write signal", () => Plain.writeSignal())
        t("write missing", () => Plain.writeMissing())
        t("write own", () => Plain.writeOwn() + "," + Plain.own + "," + root.own)
        t("shadow", () => Plain.shadow() + "," + root.declared)
        t("emit", () => Plain.emit())
        t("this", () => Plain.thisIs())
        t("nested", () => Plain.nested())
        t("via arg", () => Plain.viaArg(() => declared))
        t("call other", () => Plain.callOther())
        t("stored fn", () => { var f = Plain.readProp; return f() })
        t("fn value", () => root.fnValue())
        t("call", () => Plain.readProp.call({ declared: 1 }))
        t("child", () => inner.viaChild())
        t("child this", () => inner.childThis())
        t("own first", () => Plain.ownFirst())
        t("shadowed", () => Plain.shadowed())
        t("parent prop", () => Plain.parentProp())
        t("caller prop", () => Plain.callerProp())
        t("other read", () => other.read())
        t("other own", () => other.readOwn())
        t("other write own", () => other.writeOwn())
        t("other write", () => other.write() + "," + root.declared)
        t("other var", () => other.ownVar() + "," + Plain.own)
        t("other shadowed", () => other.shadowed())
        t("bound", () => root.bound)
        declared = 50
        t("bound after", () => root.bound)
        t("inner read", () => Plain.innerRead())
        t("inner write", () => Plain.innerWrite() + "," + root.declared)
        t("included", () => Plain.included())
        t("map", () => Plain.evalLater())
        t("typeofs", () => Plain.typeofs())
        t("type name", () => Plain.typeName())
        t("delete", () => Plain.del() + "," + root.declared)
        t("ns read", () => Plain.declared + "," + ("declared" in Plain) + "," + Plain.hasOwnProperty("declared") + "," + typeof Plain.root + "," + typeof Plain.ping)
        t("ns keys", () => Object.keys(Plain).length)
        t("instances", () => other.setOwn(41) + "," + other2.setOwn(42) + "," + other.getOwn() + "," + other2.getOwn() + "," + Plain.getOwn())
        t("instances scope", () => { other2.declared = 600; return other.read() + "," + other2.read() })
        t("same fn", () => (other.fn() === other2.fn()) + "," + (other.fn() === other.fn()))
        t("lib read", () => Lib.readProp())
        t("lib id", () => Lib.readId())
        t("lib write", () => Lib.writeProp())
    }
}
