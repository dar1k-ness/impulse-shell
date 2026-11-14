// The small ES built-ins QV4 6.12 has: String.fromCharCode/fromCodePoint/raw and
// String.prototype.codePointAt, Array.prototype.copyWithin/values/toLocaleString,
// Number.isSafeInteger/MIN_VALUE/NaN, the Math functions and constants of ES2015,
// escape/unescape, eval (indirect: global code), gc, DOMException/SQLException, the
// restricted Function.prototype.arguments/caller, signal connect on a plain function and
// Object.prototype.__defineGetter__/__defineSetter__.
// v4: ["\"\"","5","\"ABA\"","3","\"AB\"","throw RangeError","throw RangeError","throw RangeError","\"a1b2c\"","\"x-y+z\"","\"\"","throw TypeError","128512","56832","undefined","undefined","97","98","\"4,5,3,4,5\"","\"1,2,3,1,2\"","\"4,2,3,4,5\"","\"1,2,3,3,4\"","\"1,1,2,3,4\"","\"1,2,1,2,5\"","\"1,1,2\"","\"false,3,3\"","\"{\\\"0\\\":1,\\\"3\\\":1,\\\"length\\\":5}\"","\"1.5,X,,\"","true","true","false","false","false","true","5e-324","NaN","\"31,32,0,32,32,32,32\"","\"5.5,5.050000190734863,Infinity,NaN,-Infinity\"","\"12,-5,-2\"","\"9.999999999500001e-11,1.00000000005e-10,1.5430806348152437,1.1752011936438014,0.7615941559557649,1\"","\"1.3169578969248166,0.881373587019543,0.5493061443340548,1\"","\"2.302585092994046,0.4342944819032518,1.4426950408889634,0.7071067811865476\"","\"a%20b+c/%E4%u20AC%uD83D%uDE00@*_-.%7E%21%27%28%29\"","\"€äA%zz%u12%4A\"","true","\"undefinedundefined\"","3","42","undefined","\"5number\"","\"7function\"","\"8undefined\"","\"use strictundefined\"","\"undefined,5,5,undefined,undefined\"","\"6,7,2,3,9\"","\"function\"","throw SyntaxError","undefined","\"function,undefined\"","\"11,17,1\"","\"1,2,8\"","0","throw TypeError","throw TypeError","\"functionfunction\"","throw Error","\"4,x|y|z,9,undefined\"","throw TypeError"]
function t(f) {
    try {
        var v = f();
        return typeof v === "string" ? JSON.stringify(v) : String(v);
    } catch (e) {
        return "throw " + e.name;
    }
}
var holes = [1, , 3].copyWithin(0, 1);
var generic = Array.prototype.copyWithin.call({length: 5, 3: 1}, 0, 3);
var acc = {};
acc.__defineGetter__('x', function () { return 4; });
acc.__defineSetter__('y', function (v) { this.z = v; });
acc.y = 9;
var r = [
    t(function () { return String.fromCharCode(); }),
    t(function () { return String.fromCharCode(65, 66.7, 65601, 'x', NaN).length; }),
    t(function () { return String.fromCharCode(65, 66.7, 65601); }),
    t(function () { return String.fromCodePoint(0x1F600, 65).length; }),
    t(function () { return String.fromCodePoint('65', 66); }),
    t(function () { return String.fromCodePoint(-1); }),
    t(function () { return String.fromCodePoint(1.5); }),
    t(function () { return String.fromCodePoint(0x110000); }),
    t(function () { return String.raw({raw: ['a', 'b', 'c']}, 1, 2, 3); }),
    t(function () { return String.raw({raw: 'xyz'}, '-', '+'); }),
    t(function () { return String.raw({raw: []}); }),
    t(function () { return String.raw(); }),
    t(function () { return '\uD83D\uDE00x'.codePointAt(0); }),
    t(function () { return '\uD83D\uDE00x'.codePointAt(1); }),
    t(function () { return 'abc'.codePointAt(5); }),
    t(function () { return 'abc'.codePointAt(-1); }),
    t(function () { return 'abc'.codePointAt(); }),
    t(function () { return 'abc'.codePointAt('1'); }),
    t(function () { return [1, 2, 3, 4, 5].copyWithin(0, 3).join(); }),
    t(function () { return [1, 2, 3, 4, 5].copyWithin(-2).join(); }),
    t(function () { return [1, 2, 3, 4, 5].copyWithin(0, 3, 4).join(); }),
    t(function () { return [1, 2, 3, 4, 5].copyWithin(-2, -3, -1).join(); }),
    t(function () { return [1, 2, 3, 4, 5].copyWithin(1, 0).join(); }),
    t(function () { return [1, 2, 3, 4, 5].copyWithin(2, 0, 2).join(); }),
    t(function () { return [1, 2, 3].copyWithin(1, NaN).join(); }),
    t(function () { return holes.hasOwnProperty(0) + ',' + holes.length + ',' + holes[1]; }),
    t(function () { return JSON.stringify(generic); }),
    t(function () { return [1.5, {toLocaleString: function () { return 'X'; }}, null, undefined].toLocaleString(); }),
    t(function () { return [7, 8].values !== undefined; }),
    t(function () { return Number.isSafeInteger(9007199254740991); }),
    t(function () { return Number.isSafeInteger(9007199254740992); }),
    t(function () { return Number.isSafeInteger(1.5); }),
    t(function () { return Number.isSafeInteger('1'); }),
    t(function () { return Number.isSafeInteger(-0); }),
    t(function () { return Number.MIN_VALUE; }),
    t(function () { return Number.NaN; }),
    t(function () { return [Math.clz32(1), Math.clz32(0), Math.clz32(-1), Math.clz32(0.5), Math.clz32(NaN), Math.clz32(4294967296), Math.clz32()].join(); }),
    t(function () { return [Math.fround(5.5), Math.fround(5.05), Math.fround(1e40), Math.fround(NaN), 1 / Math.fround(-0)].join(); }),
    t(function () { return [Math.imul(3, 4), Math.imul(0xffffffff, 5), Math.imul(2147483647, 2)].join(); }),
    t(function () { return [Math.log1p(1e-10), Math.expm1(1e-10), Math.cosh(1), Math.sinh(1), Math.tanh(1), Math.tanh(Infinity)].join(); }),
    t(function () { return [Math.acosh(2), Math.asinh(1), Math.atanh(0.5), Math.cosh(-0)].join(); }),
    t(function () { return [Math.LN10, Math.LOG10E, Math.LOG2E, Math.SQRT1_2].join(); }),
    t(function () { return escape('a b+c/\u00e4\u20ac\uD83D\uDE00@*_-.~!\'()'); }),
    t(function () { return unescape('%u20AC%E4%41%zz%u12%4%u0041'); }),
    t(function () { return unescape(escape('\uD83D\uDE00')) === '\uD83D\uDE00'; }),
    t(function () { return escape() + unescape(); }),
    t(function () { return eval('1+2'); }),
    t(function () { return eval(42); }),
    t(function () { return eval(); }),
    t(function () { return eval('var evalVar = 5; evalVar') + typeof evalVar; }),
    t(function () { return eval('function evalFn() { return 7 }; evalFn()') + typeof evalFn; }),
    t(function () { return eval('8; let evalLet = 1') + typeof evalLet; }),
    t(function () { return eval("'use strict'; var evalStrict = 1") + typeof evalStrict; }),
    t(function () { return [eval('5; if (true) {}'), eval('5; var q2 = 2'), eval('5; {}'), eval('5; for (;false;);'), eval('5; try {} finally {}')].map(String).join(); }),
    t(function () { return [eval('if (true) { 6 }'), eval('do { 7; break } while (true)'), eval('var i2 = 0; while (i2 < 3) i2++'), eval('l: { 3; break l; }'), eval('9;;')].map(String).join(); }),
    t(function () { return (0, eval)('typeof Math.max'); }),
    t(function () { return eval('('); }),
    t(function () { return eval(''); }),
    t(function () { return typeof gc + ',' + gc(); }),
    t(function () { return DOMException.INVALID_STATE_ERR + ',' + DOMException.TYPE_MISMATCH_ERR + ',' + DOMException.INDEX_SIZE_ERR; }),
    t(function () { return SQLException.UNKNOWN_ERR + ',' + SQLException.DATABASE_ERR + ',' + SQLException.TIMEOUT_ERR; }),
    t(function () { return Object.keys(DOMException).length; }),
    t(function () { return Function.prototype.caller; }),
    t(function () { return (function () {}).arguments; }),
    t(function () { return typeof Function.prototype.connect + typeof Function.prototype.disconnect; }),
    t(function () { return (function () {}).connect(function () {}); }),
    t(function () { return acc.x + ',' + Object.keys(acc).join('|') + ',' + acc.z + ',' + acc.y; }),
    t(function () { return ({}).__defineGetter__('x', 1); }),
];
var result = JSON.stringify(r);
