// %Object.prototype% (ECMA-262 §20.1.3): every object inherits its methods past its own chain.
// v4: ["[object Object]",true,true,true,"function",true,false,false,"[object Array]","[object Null]","[object Undefined]","[object Number]","[object String]","[object Boolean]","[object Function]","[object Date]","[object RegExp]","[object Error]","[object Map]","[object Set]","[object Arguments]",true,true,true,false,false,false,true,false,true,false,true,"[object Object]","own",true,true,true,true,true,true,false,false,"undefined",false,"[object Object]",true,true,false,5,true,"s",false,"function","function","[object Object]","[object Object]","[object Map]","own",true,"TypeError","TypeError","TypeError","TypeError",0,0,true]
function err(f) { try { return f() } catch (e) { return e.name } }
function C() {}
var bare = Object.create(null);
var o = { a: 1 };
var ts = Object.prototype.toString;
var result = JSON.stringify([
  ({}).toString(), "toString" in {}, "valueOf" in o, "hasOwnProperty" in o, typeof o.isPrototypeOf,
  o.hasOwnProperty("a"), o.hasOwnProperty("toString"), ({}).valueOf() === undefined,
  ts.call([]), ts.call(null), ts.call(undefined), ts.call(1), ts.call("s"), ts.call(true),
  ts.call(function () {}), ts.call(new Date(0)), ts.call(/x/), ts.call(new Error("e")),
  ts.call(new Map()), ts.call(new Set()), (function () { return ts.call(arguments) })(),
  Object.prototype.isPrototypeOf(o), Object.prototype.isPrototypeOf(new C()),
  C.prototype.isPrototypeOf(new C()), Object.prototype.isPrototypeOf(bare),
  Object.prototype.isPrototypeOf(Object.prototype), "s".isPrototypeOf({}),
  o.propertyIsEnumerable("a"), o.propertyIsEnumerable("toString"),
  [1].propertyIsEnumerable(0), [1].propertyIsEnumerable("length"), [1].hasOwnProperty(0),
  ({}).toLocaleString(), ({ toString: function () { return "own" } }).toLocaleString(),
  o.constructor === Object, Object.prototype.constructor === Object, ({}) instanceof Object,
  [] instanceof Object, new C() instanceof Object, (function () {}) instanceof Object,
  bare instanceof Object, "toString" in bare, typeof bare.toString, "x" in bare,
  ts.call(bare), Object.prototype.hasOwnProperty.call("ab", 1),
  Object.prototype.hasOwnProperty.call("ab", "length"), Object.prototype.hasOwnProperty.call("ab", 2),
  (5).valueOf(), true.valueOf(), "s".valueOf(), (5).hasOwnProperty("x"), typeof "s".hasOwnProperty,
  typeof (1).isPrototypeOf, "" + {}, String({}), String(new Map()),
  ({ toString: function () { return "own" } }).toString(), ({ toString: 1 }).hasOwnProperty("toString"),
  err(function () { return Object.prototype.valueOf.call(null) }),
  err(function () { return Object.prototype.hasOwnProperty.call(undefined, "x") }),
  err(function () { return Object.create(1) }), err(function () { return bare + "" }),
  Object.keys(Object.prototype).length, (function () { var n = 0; for (var k in {}) n++; return n })(),
  String(function f() {}) === (function f() {}).toString()
]);
