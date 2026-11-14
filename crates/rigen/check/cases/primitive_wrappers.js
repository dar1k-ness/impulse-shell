// `new Number/Boolean/String` make objects with [[NumberData]]/[[BooleanData]]/[[StringData]]
// while a plain call converts (ECMA-262 §21.1.1.1, §20.3.1.1, §22.1.1.1), and non-strict code
// binds a primitive `this` boxed while strict code keeps it (OrdinaryCallBindThis, §10.2.1.2).
// v4: ["object","object","object","number","boolean","string",6,false,true,false,true,1,true,"abc",2,"b",["0","1"],true,false,true,["a","b"],["a","b"],"5","{\"a\":\"ab\",\"b\":false,\"c\":5}","[object Number]","[object Boolean]","[object String]","[object Number]",5,false,"ab","5.00","ff","AB","false",true,false,true,true,true,"object",true,"object",true,0,"","TypeError","TypeError","TypeError","5","ab21","object:true:false:false","object:false:true:false","object:false:false:true","number","undefined","undefined","object","number","object","object","xy!",2,"object:true:false:false","object:true:false:false","object"]
var n = new Number(5), b = new Boolean(false), s = new String("ab");
var tag = function (v) { return Object.prototype.toString.call(v) };
function sloppy() { return typeof this + ":" + (this instanceof Number) + ":" + (this instanceof String) + ":" + (this instanceof Boolean) }
function strict() { "use strict"; return typeof this }
Number.prototype.kindS = function () { return typeof this };
Number.prototype.kindT = function () { "use strict"; return typeof this };
function brand(f) { try { return f() } catch (e) { return e.name } }
var arrowThis = (function () { return (() => typeof this)() }).call(7);
var boxed = (function () { return this }).call("xy");
var result = JSON.stringify([
    typeof n, typeof b, typeof s, typeof Number("5"), typeof Boolean(0), typeof String(1),
    n + 1, n === 5, n == 5, n == new Number(5), !!b, b ? 1 : 2, b == false, s + "c", s.length, s[1],
    Object.keys(s), s.hasOwnProperty("0"), s.propertyIsEnumerable("length"), "1" in s, [...s], Array.from(s),
    JSON.stringify(n), JSON.stringify({ a: s, b: b, c: n }),
    tag(n), tag(b), tag(s), tag(5), n.valueOf(), b.valueOf(), s.valueOf(), n.toFixed(2),
    new Number(255).toString(16), s.toUpperCase(), b.toString(),
    n instanceof Number, 5 instanceof Number, s instanceof Object, n.constructor === Number,
    Number.prototype.isPrototypeOf(n), typeof Object(5), Object(5) instanceof Number,
    typeof new Object("q"), new Object(true) instanceof Boolean, new Number() + 0, new String() + "",
    brand(() => Number.prototype.valueOf.call(s)), brand(() => Boolean.prototype.toString.call(n)),
    brand(() => String.prototype.valueOf.call(n)), Number.prototype.toString.call(n),
    (function () { s[0] = "z"; s.length = 9; delete s[1]; s.extra = 1; return s[0] + s[1] + s.length + s.extra })(),
    sloppy.call(5), sloppy.call("s"), sloppy.call(true), strict.call(5), strict.call(undefined), strict(),
    (5).kindS(), (5).kindT(), arrowThis, typeof boxed, boxed + "!", boxed.length,
    sloppy.apply(1), sloppy.bind(2)(), [0].map(function () { return typeof this }, 3)[0]]);
