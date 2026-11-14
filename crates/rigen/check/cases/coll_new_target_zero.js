// Map and Set constructors make their object from NewTarget (OrdinaryCreateFromConstructor,
// ECMA-262 §24.1.1.1 step 2, §24.2.2.1 step 2) before AddEntriesFromIterable reads `set`/`add`,
// so a subclass's own adder receives the initial entries; `Map.prototype.set` and
// `Set.prototype.add` store a -0 key as +0 (§24.1.3.9 step 5, §24.2.3.1 step 4).
var log = [];
class M extends Map { set(k, v) { log.push("M" + k); return super.set(k, v) } }
class S extends Set { add(v) { log.push("S" + v); return super.add(v) } }
var m = new M([[1, "a"], [3, "b"]]);
var s = new S([5, 7]);
var mz = new Map(); mz.set(-0, 1);
var sz = new Set(); sz.add(-0);
var mi = new Map([[-0, 2]]), si = new Set([-0]);
var fe = [];
mz.forEach(function (v, k) { fe.push(Object.is(k, 0)) });
sz.forEach(function (v, k) { fe.push(Object.is(v, 0), Object.is(k, 0)) });
var result = JSON.stringify([
    log.join(), m instanceof M, m.size, m.get(3), s instanceof S, s.size,
    Object.is(mz.keys().next().value, 0), Object.is(mz.entries().next().value[0], 0),
    Object.is(sz.values().next().value, 0), Object.is(sz.entries().next().value[1], 0),
    Object.is(mi.keys().next().value, 0), Object.is(si.values().next().value, 0),
    fe.join(), mz.has(0), mz.get(-0), sz.has(0)
]);
// v4: ["M1,M3,S5,S7",true,2,"b",true,2,true,true,true,true,true,true,"true,true,true",true,1,true]
