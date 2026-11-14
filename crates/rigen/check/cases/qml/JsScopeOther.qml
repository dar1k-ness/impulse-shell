import QtQuick
import "js_scope_plain.js" as P2
Item {
    property int declared: 500
    property int onlyChild: 4
    property string shadowedName: "other"
    function read() { return P2.readProp() }
    function readOwn() { return P2.callerProp() }
    function writeOwn() { return P2.writeCallerProp() + "," + onlyChild }
    function write() { P2.writeProp(); return declared }
    function ownVar() { return P2.writeOwn() + "," + P2.own }
    function setOwn(v) { return P2.setOwn(v) }
    function getOwn() { return P2.getOwn() }
    function fn() { return P2.readProp }
    function shadowed() { return P2.shadowed() }
}
