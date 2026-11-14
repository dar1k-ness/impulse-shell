// Computed-key function names and the non-writable, configurable `name`/`length` of functions.
// v4: ["ck","ck2","2","ck","ckm","g0","true","<>false","0false","<>","true2","0","TypeError","TypeError","TypeError","TypeError","TypeError","TypeError","TypeError","TypeError","TypeError","TypeError","TypeError","TypeError","5","TypeError","TypeError","1undefined"]
var out = [];
function t(f) {
    try {
        out.push(String(f()));
    } catch (e) {
        out.push(e.name);
    }
}
var k = "ck";
var o = { [k]: function () {}, [k + "2"]: () => 1, get [k + "g"]() { return 1 }, [1 + 1]: function () {} };
t(function () { return o.ck.name });
t(function () { return o.ck2.name });
t(function () { return o[2].name });
class C { static [k]() {} [k + "m"]() {} }
t(function () { return C.ck.name });
t(function () { return C.prototype.ckm.name });
t(function () { var f = function g() {}; f.name = 1; f.length = 9; return f.name + f.length });
t(function () { var f = function g(a) {}; return delete f.name });
t(function () { var f = function g(a) {}; delete f.name; return "<" + f.name + ">" + f.hasOwnProperty("name") });
t(function () { var f = function g(a) {}; delete f.length; return f.length + "" + f.hasOwnProperty("length") });
t(function () { var f = function g(a) {}; delete f.name; f.name = "x"; return "<" + f.name + ">" });
t(function () { return Math.max.hasOwnProperty("length") + "" + Math.max.length });
t(function () { delete Math.max.length; return Math.max.length });
t(function () { "use strict"; var f = function g() {}; f.name = 1; return 1 });
t(function () { "use strict"; var f = function g(a) {}; f.length = 5; return 1 });
t(function () { "use strict"; var f = function g() {}; f["name"] = 1; return 1 });
t(function () { "use strict"; var f = function g() {}; f.name += "x"; return 1 });
t(function () { "use strict"; var f = function g() {}; f.length++; return 1 });
t(function () { "use strict"; var f = function g() {}; delete f.name; f.name = 1; return 1 });
t(function () { "use strict"; Math.max.name = 1; return 1 });
t(function () { "use strict"; (class A {}).name = 1; return 1 });
t(function () { "use strict"; o.ck.name = 1; return 1 });
t(function () { "use strict"; var f = function g() {}; [f.name] = [1]; return 1 });
t(function () { "use strict"; var p = { get g() { return 1 } }; p.g = 2; return 1 });
t(function () { "use strict"; var s = "abc"; s.x = 1; return 1 });
t(function () { "use strict"; var p = { a: 1 }; p.a = 2; p.b = 3; return p.a + p.b });
t(function () { "use strict"; return (function () { var f = function g() {}; f.name = 1; return "ok" })() });
t(function () { var f = function g() {}; return (() => { "use strict"; f.name = 1; return "ok" })() });
t(function () { var p = { get g() { return 1 } }; p.g = 2; var s = "abc"; s.x = 1; return p.g + "" + s.x });
var result = JSON.stringify(out);
