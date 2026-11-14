.import "js_scope_inner.js" as Inner
var own = 1
function readProp() { return declared }
function readCpp() { return width }
function readId() { return typeof root + "," + (root === inner.parent) }
function readInner() { return inner.innerProp }
function readMethod() { return ping() }
function readMissing() { return typeof nowhere }
function readMissingThrow() { return nowhere }
function writeProp() { declared = 21; return declared }
function writeCpp() { width = 33; return width }
function writeCompound() { declared += 2; return declared }
function writeIncr() { declared++; return declared }
function writeId() { root = 3; return 0 }
function writeMethod() { ping = 3; return typeof ping }
function writeSignal() { poked = 3; return typeof poked }
function writeMissing() { nowhere2 = 1; return typeof nowhere2 }
function writeOwn() { own = 5; return own }
function shadow() { var declared = 100; declared = 101; return declared }
function emit() { poked(); return count }
function thisIs() { return typeof this + "," + (this === root) }
function thisProp() { return this.declared }
function nested() { return (function() { return (() => declared + 1)() })() }
function viaArg(f) { return f() }
function callOther() { return readProp() + 1 }
function callerProp() { return onlyChild }
function writeCallerProp() { onlyChild = 9; return onlyChild }
function shadowed() { return typeof shadowedName + "," + shadowedName }
function ownFirst() { return typeof own + "," + own }
function parentProp() { return typeof childOnly }
var shadowedName = "js"
var atLoad = (function() { try { return typeof declared + "," + typeof root + "," + typeof width + "," + typeof ping } catch (e) { return String(e) } })()
var atLoadThis = typeof this
function innerRead() { return Inner.read() }
function innerWrite() { return Inner.write() }
function included() { Qt.include("js_scope_included.js"); return incRead() }
function setOwn(v) { own = v; return own }
function getOwn() { return own }
function evalLater() { return [1, 2].map(function(x) { return x + declared }).join("|") }
function typeofs() { return [typeof Rectangle, typeof Qt, typeof Plain, typeof Inner, typeof parent, typeof objectName, typeof console].join("|") }
function typeName() { return Rectangle.Fit === undefined ? "no" : "yes" }
function del() { return delete declared }
