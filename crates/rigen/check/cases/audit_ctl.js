// Audit case, control flow: switch, per-iteration loop bindings, updates of members with
// side-effecting keys, labels, `finally`, arrows, computed keys, destructuring parameters,
// exceptions, class accessors. The pin is ECMA-262's result (Node 22): Qt 6.12's V4 has no
// temporal dead zone across switch cases (`sw2`) and differs in `dp`.
// expect: 1d2,2,3,d2,5,5,ReferenceError,ReferenceError,a1,b2,z2,after1,def,1,1,3,5,0,0,0,1,0,2,0,1,2,a,b,0,2,4,0,1,3,4,1,2,1,3,2,6,4,36,5,6,number,NaN,true,get,set3,get,set2,1,get,set0,0,true,true,false,true,3,2,true,false,3,00,f,f,10,f,f,f,in,r2,2,a3,1:T,2,a12cd,abcd,a12cdsm,1,2,2,0,2,5,6,6,2,8,0,x2,f1,2,fin3,t,undefined,7,720,BAg,AvB
var result = [];
function p(x) { result.push(x); }
// switch: default in the middle, fallthrough, lexical decl captured by closure, TDZ across cases
function sw(x) { var o = []; switch (x) { case 1: o.push("1"); default: o.push("d"); case 2: o.push("2"); break; case 3: o.push("3"); } return o.join(""); }
p(sw(1)); p(sw(2)); p(sw(3)); p(sw(9));
function sw2(x) { var fs = []; switch (x) { case 0: let a = 10; fs.push(function () { return a; }); case 1: try { fs.push(function () { return a; }); a = 5; } catch (e) { p(e.name); } break; } return fs.map(function (f) { try { return f(); } catch (e) { return e.name; } }).join(","); }
p(sw2(0)); p(sw2(1));
function sw3(v) { var n = 0; switch (v) { case (n++, 1): return "a" + n; case (n++, 2): return "b" + n; default: return "z" + n; } }
p(sw3(1)); p(sw3(2)); p(sw3(7));
function sw4(v) { out: switch (v) { case 1: for (var i = 0; i < 3; i++) { if (i == 1) break out; } return "no"; default: return "def"; } return "after" + i; }
p(sw4(1)); p(sw4(2));
p((function () { switch (typeof null) { case "object": return 1; } return 2; })());
// per-iteration bindings with mutation in body and closures in init/test/update
var fs = []; for (let i = 0; i < 6; i++) { fs.push(function () { return i; }); i++; }
p(fs.map(function (f) { return f(); }).join(","));
var gs = []; for (let i = 0, f0 = function () { return i; }; i < 3; i++) { gs.push(f0); gs.push(function () { return i; }); }
p(gs.map(function (f) { return f(); }).join(","));
var hs = []; for (let i = 0; hs.push(function () { return i; }), i < 2; i++) { }
p(hs.map(function (f) { return f(); }).join(","));
var ks = []; for (let k in { a: 1, b: 2 }) ks.push(function () { return k; });
p(ks.map(function (f) { return f(); }).join(","));
var ws = []; var wi = 0; while (wi < 3) { let c = wi * 2; ws.push(function () { return c; }); wi++; if (wi == 2) continue; }
p(ws.map(function (f) { return f(); }).join(","));
var ds = []; var di = 0; do { const c = di; ds.push(function () { return c; }); if (di == 1) { di += 2; continue; } di++; } while (di < 5);
p(ds.map(function (f) { return f(); }).join(","));
// update / compound on members with side-effecting keys
var cnt = 0; function k() { cnt++; return "x"; } var o = { x: 1 };
p(o[k()]++); p(o.x); p(cnt); p(++o[k()]); p(cnt); o[k()] += o[k()]; p(o.x); p(cnt);
o[k()] **= 2; p(o.x); var nn = "5"; p(nn++); p(nn); p(typeof nn); var un; p(un++); p(isNaN(un));
var s2 = { get v() { p("get"); return 1; }, set v(x) { p("set" + x); } }; s2.v += 2; p(s2.v++); p(--s2.v);
// in / delete / void / comma / conditional
p("x" in o); p(delete o.x); p("x" in o); p(void 0 === undefined); p((1, 2, 3)); p(true ? false ? 1 : 2 : 3);
var arr = [1, 2, 3]; p(delete arr[1]); p(1 in arr); p(arr.length);
// labelled blocks, continue in finally, nested loops
var log = []; outer: for (var a = 0; a < 3; a++) { inner: for (var b = 0; b < 3; b++) { try { if (b == 1) continue outer; if (a == 2) break outer; log.push(a + "" + b); } finally { log.push("f"); } } }
p(log.join(","));
blk: { p("in"); if (cnt) break blk; p("never"); }
function fin() { for (var i = 0; i < 3; i++) { try { return "r" + i; } finally { if (i < 2) continue; } } return "end"; }
p(fin());
function fin2() { l: try { return 1; } finally { break l; } return 2; }
p(fin2());
function fin3() { var i = 0; while (true) { try { try { throw new Error("a"); } finally { i++; if (i < 3) continue; } } catch (e) { return e.message + i; } } }
p(fin3());
// arrows: this/arguments lexical; nested
function ar() { var f = () => arguments.length + ":" + (this && this.t); return f(1, 2, 3); }
p(ar.call({ t: "T" }, 1)); var oo = { t: 1, m() { return [1].map(x => this.t + x)[0]; } }; p(oo.m());
// computed keys and evaluation order in object literal / class
var ord = []; function e(x) { ord.push(x); return x; }
var lit = { [e("a")]: e(1), b: e(2), [e("c")]() { }, get [e("d")]() { return 4; } }; p(ord.join("")); p(Object.keys(lit).join(""));
class C { static [e("s")]() { } [e("m")]() { } } p(ord.join(""));
// destructuring params
function dp({ a = 1, b: { c = a + 1 } = {} } = {}, [d = c, ...r] = [], z = d + r.length) { return [a, c, d, r.length, z].join(","); }
p(dp()); p(dp({ a: 5 }, [undefined, 1, 2])); p(dp.length);
// exceptions: catch param destructuring, rethrow, finally overriding
function ex() { try { throw { m: "x", n: [1, 2] }; } catch ({ m, n: [, q] }) { return m + q; } }
p(ex());
function ex2() { try { try { throw 1; } catch (e) { throw e + 1; } finally { p("f1"); } } catch (e2) { return e2; } }
p(ex2());
function ex3() { try { return "t"; } finally { p("fin3"); } }
p(ex3());
// typeof undeclared, closures over catch var, recursion via named fn expr
p(typeof nope); var cf; try { throw 7; } catch (ce) { cf = function () { return ce; }; } p(cf());
p((function fact(n) { return n < 2 ? 1 : n * fact(n - 1); })(6));
// getter/setter on classes & super property assignment
class A { get g() { return "Ag"; } set g(v) { this._g = "A" + v; } }
class B extends A { get g() { return "B" + super.g; } set g(v) { super.g = v + "B"; } }
var bb = new B(); bb.g = "v"; p(bb.g); p(bb._g);
