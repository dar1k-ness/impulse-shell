// v4: 0|0,1|a||0,ap,op,name|ap,op,name|0,1,sp,op,push,name|a,op,push,length,name|op,ap,push,length,name|np,op,push,length,name|bp,op,push,length,name|fp,op,push|fp,op,push|op,push,length,name|op,push,length,name|op,push,length,name|op,push,length,name|op,push,length|0,op,push,name||x,ap,op,push,length,name|ap,op,name|sp,op,push,name|op,push,length,name|0,1,sp,op,push,name|np,op,push,length,name|bp,op,push,length,name|/
// for-in (ECMA-262 EnumerateObjectProperties on ToObject(v)) walks the implicit built-in
// prototypes too: an enumerable key added to Array/String/Number/Boolean/Function/Object.prototype
// shows unless an own or nearer key, enumerable or not (length, name, push), shadows it.
function ks(v) { var a = []; for (var k in v) a.push(k); return a.join(); }
var before = [ks([5]), ks("ab"), ks({ a: 1 }), ks(7)].join("|");
Array.prototype.ap = 1;
String.prototype.sp = 2;
Number.prototype.np = 3;
Boolean.prototype.bp = 4;
Function.prototype.fp = 5;
Object.prototype.op = 6;
Object.prototype.push = 7;
Object.prototype.length = 8;
Object.prototype.name = 9;
var p = { x: 1, ap: 0 };
var result = [
  before, ks([5]), ks([]), ks("ab"), ks({ a: 1 }), ks({ op: 0, ap: 0 }), ks(7), ks(true),
  ks(function () {}), ks(Array), ks(Math), ks(new Date(0)), ks(/x/), ks(new Map()),
  ks(new Error("e")), ks((function () { return arguments })(1)), ks(Object.create(null)),
  ks(Object.create(p)), ks(Array.prototype), ks(String.prototype), ks(Object.prototype),
  ks(new String("ab")), ks(new Number(1)), ks(new Boolean(false)),
  ks(null) + "/" + ks(undefined)
].join("|");
