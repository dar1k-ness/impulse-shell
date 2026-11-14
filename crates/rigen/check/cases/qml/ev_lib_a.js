.import "ev_lib_b.js" as B
.import QtQuick as QQ
.import QtQml 2.15 as QM
var x = 1
let lx = 3
const cx = 4
class K {}
var arr = [1]
var und
function fa() { return B.y }
function qq() { return [typeof QQ, typeof QQ.Item, typeof QQ.Text, typeof QM, typeof QM.Timer, typeof QQ.Timer, typeof QM.Item, typeof QQ.Item.TopLeft, typeof Item, typeof Qt, typeof Qt.rgba, Object.keys(QQ).length, "Item" in QQ, typeof QQ.Nope] }
function bump() { x++; return x }
function lex() { return [lx, cx, typeof K] }
function setLate() { late = 5; return typeof late }
function getX() { return x }
function self() { return this }
function decl() { var inner = 1; return typeof inner }
function redefine() { fa = function() { return "new" }; return fa() }
