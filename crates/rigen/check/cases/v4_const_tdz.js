// Assigning a const throws TypeError; a let/const/class binding used before its declaration
// throws ReferenceError (temporal dead zone), from closures, typeof and switch cases too.
// v4: ["TypeError",1,"TypeError","TypeError","TypeError","TypeError","TypeError","TypeError","ok:5","ReferenceError","ReferenceError","ReferenceError","ReferenceError","ReferenceError","ok:1","ReferenceError","ReferenceError","ReferenceError","ReferenceError","ok:undefined","ok:2","ok:4"]
function kind(f) { try { return "ok:" + f() } catch (e) { return e.name } }
const top = 1;
var r = [];
r.push(kind(function () { top = 2 }), top);
r.push(kind(function () { const c = 1; c += 1; return c }));
r.push(kind(function () { const c = 1; c++; return c }));
r.push(kind(function () { const c = 1; [c] = [2]; return c }));
r.push(kind(function () { for (const i of [1]) { i = 5 } return 0 }));
r.push(kind(function () { const c = 1; function g() { c = 3 } g(); return c }));
r.push(kind(function () { class K { m() { K = 1 } } new K().m(); return 0 }));
r.push(kind(function () { var s = 0; const c = 1; try { c = (s = 5) } catch (e) { return s } }));
r.push(kind(function () { var y = x; let x = 1; return y }));
r.push(kind(function () { var t = typeof x; let x = 1; return t }));
r.push(kind(function () { x = 2; let x = 1; return x }));
r.push(kind(function () { let x = x + 1; return x }));
r.push(kind(function () { function g() { return x } var v = g(); let x = 1; return v }));
r.push(kind(function () { function g() { return x } let x = 1; return g() }));
r.push(kind(function () { let x = 1; { var y = x; let x = 2 } return y }));
r.push(kind(function () { var o = new C(); class C {} return o }));
r.push(kind(function () { switch (1) { case 0: let z = 1; break; case 1: return z } }));
r.push(kind(function () { for (let i = 0; i < 2; i++) { var v = j; let j = 1 } return v }));
r.push(kind(function () { let u; return typeof u }));
r.push(kind(function () { let a = 1; a = 2; return a }));
r.push(kind(function () { let f = function () { return n }; let n = 4; return f() }));
var result = JSON.stringify(r);
