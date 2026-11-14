// ECMA-262 §13.3.9.1: a parenthesized optional chain is still a Reference, so `(o?.f)()`
// calls with `this` = o (test262 optional-chaining/optional-call-preserves-this.js); a
// short-circuit makes the callee `undefined` (arguments still evaluated, then TypeError).
// v4: [["true","true","true","true","true","TypeError","TypeError","undefined","true","undefined","TypeError","2","2","bc"],["1","k1","2","3","5","k5","7","8"]]
var log = [];
function arg(x) { log.push(x); return x; }
var o = {
    f() { return this === o; },
    g: { h() { return this === o.g; } },
    k: function (x) { log.push("k" + x); return this; },
    arr: [1],
};
var z = null;
var r = [];
r.push((o?.f)());
r.push((o?.g.h)());
r.push((o.g?.h)());
r.push((o?.["f"])());
r.push((o?.k)(arg(1)) === o);
try { (z?.f)(arg(2)); r.push("no"); } catch (e) { r.push(e.name); }
try { (z?.[arg("key")])(arg(3)); r.push("no"); } catch (e) { r.push(e.name); }
r.push((z?.f)?.(arg(4)));
r.push((o?.k)?.(arg(5)) === o);
r.push((o?.nope)?.(arg(6)));
try { (o?.nope)(arg(7)); r.push("no"); } catch (e) { r.push(e.name); }
r.push((o?.arr.push)(arg(8)));
r.push(o.arr.length);
var s = "abc";
r.push((s?.slice)(1));
var result = JSON.stringify([r.map(String), log.map(String)]);
