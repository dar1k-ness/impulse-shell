// Audit case, scopes: hoisting, temporal dead zone, closures, shadowing, parameter scope.
// The pin is ECMA-262's result (Node 22); Qt 6.12's V4 differs in several items. The item
// where this runtime follows V4 (a function declared in a block) is in audit_v4_ways.js.
// expect: ["1,3","10,10","ab,bb","2,12","ReferenceError","ReferenceError","ReferenceError","TypeError1","ReferenceError","ReferenceError","ReferenceError",1,2,"1,5","function","decl",3,9,1,1,"undefined",120,"function","TypeError","ok",2,2,12,1,1]
var log = [];
var fs = [];
for (let i = 0; i < 3; i++) { fs.push(() => i); i++; }
log.push(fs.map(f => f()).join());
fs = [];
for (let i = 0, j = 10; i < 2; i++, j--) fs.push(() => i + j);
log.push(fs.map(f => f()).join());
fs = [];
for (var k in { a: 1, b: 2 }) { let c = k; fs.push(() => c + k); }
log.push(fs.map(f => f()).join());
fs = [];
for (const [a, b] of [[1, 2], [3, 4]]) fs.push(() => a * b);
log.push(fs.map(f => f()).join());
function tdz1() { try { x; } catch (e) { return e.name; } let x = 1; }
function tdz2() { let f = () => y; try { f(); } catch (e) { return e.name; } let y = 1; }
function tdz3() { try { z = 5; } catch (e) { return e.name; } const z = 1; }
function tdz4() { const c = 1; try { c = 2; } catch (e) { return e.name + c; } }
function tdz5(a = b, b = 1) { return a; }
function tdz6() { switch (1) { case 0: let q = 1; case 1: try { return q; } catch (e) { return e.name; } } }
function tdz7() { try { return typeof tt; } catch (e) { return e.name; } let tt; }
log.push(tdz1(), tdz2(), tdz3(), tdz4(), (function(){ try { return tdz5(); } catch (e) { return e.name; } })(), tdz6(), tdz7());
function sh(a) { var a; return a; }
function sh2(a) { var a = 2; return arguments[0]; }
function sh3(a, a2 = () => a) { var a = 5; return a2() + "," + a; }
function sh4() { return typeof inner; function inner() {} }
function sh6(f) { return f(); function f() { return "decl"; } }
function sh7() { var arguments = 3; return arguments; }
function sh8(x) { x = 9; return arguments[0]; }
function sh9(x) { "use strict"; x = 9; return arguments[0]; }
function sh10(x, y = 1) { x = 9; return arguments[0]; }
log.push(sh(1), sh2(1), sh3(1), sh4(), sh6(() => "arg"), sh7(), sh8(1), sh9(1), sh10(1));
var e1 = (function () { try { throw 1; } catch (e) { var e = 2; } return e; })();
log.push(String(e1));
var nf = function fact(n) { return n <= 1 ? 1 : n * fact(n - 1); };
log.push(nf(5));
var nf2 = function self() { self = 5; return typeof self; };
log.push(nf2());
(function () { var self2 = function me() { "use strict"; try { me = 1; } catch (e) { log.push(e.name); } }; self2(); })();
function lbl() { a: { b: { break a; } log.push("no"); } return "ok"; }
log.push(lbl());
var gg = 1; { let gg = 2; { var hh = gg; } } log.push(hh);
function cl() { var r = []; for (var i = 0; i < 2; i++) { let j = i; r.push(function () { return j++; }); } return r[0]() + r[0]() + r[1](); }
log.push(cl());
function dflt(a, b = a + 1, c = () => a + b) { a = 10; return c(); }
log.push(dflt(1));
function dfl2(a = 1) { var a; return a; } log.push(dfl2());
function dfl3(a = 1, f = () => a) { var a; a = 7; return f(); } log.push(dfl3());
var result = JSON.stringify(log);
