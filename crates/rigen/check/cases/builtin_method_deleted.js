// A method deleted from a built-in prototype is looked up further, on %Object.prototype%
// (OrdinaryGet, ECMA-262 §10.1.8.1): a function without %Function.prototype%.toString
// converts to a string with %Object.prototype%.toString.
// v4: [[true,"function",true,"[object Function]","[object Function]1","[object Function]","[object Function]",true,"function",false,true,"[object Function]",1,true,true],[8,"[object Function]",true,true,"function"],[true,"function",true,"[object Array]","[object Array]","1,2","function","1,2"],[true,"function","[object RegExp]",true,true,"/x/g"]]
var out = [];
function t(fn) {
    try { out.push(fn()); } catch (e) { out.push("THROWS " + e.name); }
}
t(function () {
    var f = function () {};
    var own = Function.prototype.toString;
    var d = delete Function.prototype.toString;
    var r = [d, typeof f.toString, f.toString === Object.prototype.toString, String(f), f + 1, "" + f, [f].join(),
        String(f) == Object.prototype.toString.call(f), typeof Function.prototype.toString,
        Function.prototype.hasOwnProperty("toString"), "toString" in f, String(Math.max), ({ "[object Function]": 1 })[f]];
    Function.prototype.toString = own;
    return r.concat([String(f) == own.call(f), f.toString === own]);
});
t(function () {
    var f = function () {};
    f.valueOf = function () { return 7; };
    var own = Function.prototype.toString;
    delete Function.prototype.toString;
    var r = [f + 1, String(f), f == 7, f.call === Function.prototype.call, typeof f.bind];
    Function.prototype.toString = own;
    return r;
});
t(function () {
    var a = [1, 2];
    var own = Array.prototype.toString;
    var d = delete Array.prototype.toString;
    var r = [d, typeof a.toString, a.toString === Object.prototype.toString, String(a), a + "", a.join(), typeof Array.prototype.toString];
    Array.prototype.toString = own;
    return r.concat([String(a)]);
});
t(function () {
    var re = /x/g;
    var own = RegExp.prototype.toString;
    var d = delete RegExp.prototype.toString;
    var r = [d, typeof re.toString, String(re), re.toString === Object.prototype.toString, re.test("x")];
    RegExp.prototype.toString = own;
    return r.concat([String(re)]);
});
var result = JSON.stringify(out);
