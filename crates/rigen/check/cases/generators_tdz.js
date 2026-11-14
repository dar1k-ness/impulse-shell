// Generators split at their yields keep let/const/class semantics (temporal dead zone,
// const assignment errors, per-iteration bindings) and evaluate yields inside optional chains
// and destructuring defaults in order (ECMA-262 §9.1.1.1, §13.3.9, §8.6.2).
// v4: ReferenceError | ReferenceError | ReferenceError | TypeError | TypeError | TypeError | TypeError | ReferenceError | TypeError | ReferenceError | ReferenceError | ok:3 | ok:1 | ok:0,11,22 | ok:x | ok:false | ["arg","bundefinedtrue",null,null,"c","bundefinedtrue"] | {"done":true} | ["da","undefined|2"] | ["dp","key","undefined|dq"] | ["x","undefined|1,2"] | closed | ["1,2"] | closed | ok:1
function t(g) { try { return JSON.stringify([...g]) } catch (e) { return e.name } }
function K(f) { var it = f(); try { var r = it.next(); while (!r.done) r = it.next(r.value); return "ok:" + r.value } catch (e) { return e.name } }
var r = [];
r.push(K(function* () { yield 1; x = 2; let x = 1; return x }));
r.push(K(function* () { yield 1; var y = x; let x = 1; return y }));
r.push(K(function* () { yield 1; var s = typeof x; let x = 1; return s }));
r.push(K(function* () { const c = yield 1; c = 2; return c }));
r.push(K(function* () { const c = 1; yield c; c++; return c }));
r.push(K(function* () { const c = 1; yield c; [c] = [3]; return c }));
r.push(K(function* () { const c = 1; yield c; function f() { c = 3 } f(); return c }));
r.push(K(function* () { let x = 1; { yield x; var v = x; let x = 2; } return v }));
r.push(K(function* () { for (const i of [1, 2]) { yield i; i = 5 } return 0 }));
r.push(K(function* () { var o = yield new C(); class C {} return 1 }));
r.push(K(function* () { function f() { return z } yield f(); let z = 3; return 0 }));
r.push(K(function* () { function f() { return z } let z = 3; yield 1; return f() }));
r.push(K(function* () { let a = 1; a = yield a; return a }));
r.push(K(function* () { var fs = []; for (let i = 0; i < 3; i++) { yield i; let j = i * 10; fs.push(() => i + j) } return fs.map(f => f()).join() }));
r.push(K(function* () { switch (yield 1) { case 1: let z = 1; break; default: return z } return "x" }));
r.push(K(function* () { yield 0; return delete x; let x }));
var o = { a: { b: function (v) { return "b" + v + (this === o.a) } }, n: null };
r.push(t((function* () { yield o.a?.b(yield "arg"); yield o.n?.b(yield "never"); yield o.n?.[yield "k"]; yield o?.a.b?.(yield "c") })()));
var g1 = (function* () { return o.n?.x.y(yield 1) })(); r.push(JSON.stringify(g1.next()));
r.push(t((function* () { var [a = yield "da", b = yield "db"] = [undefined, 2]; yield a + "|" + b })()));
r.push(t((function* () { var { p = yield "dp", [yield "key"]: q = "dq" } = { key: 1, z: 9 }; yield p + "|" + q })()));
r.push(t((function* () { let a, b; [a = yield "x", ...b] = [undefined, 1, 2]; yield a + "|" + b })()));
function* closing() { try { yield 1; yield 2; yield 3 } finally { r.push("closed") } }
r.push(t((function* () { var [a, b = yield "mid"] = closing(); yield a + "," + b })()));
r.push(K(function* () { var [a = (yield 1, null.x)] = closing(); return a }));
var result = r.join(" | ");
