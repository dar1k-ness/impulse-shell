// v4: [false,"[object Map Iterator]","[object Set Iterator]",true,true,true,true,"entries","values",0,[{"value":[1,"a"],"done":false},{"value":[2,"b"],"done":false},{"done":true},{"done":true}],[["x","x"],["y","y"]],["x","y"],["a","b"],[1,2,3,4],[1,3],{"done":true},[1,2,3],["TypeError","TypeError","TypeError","TypeError","TypeError"],[0],[1,2,3],"[object Array Iterator]",false,[[0,5],[1,6]],[0,1],[[1,2]]]
var m = new Map([[1, "a"], [2, "b"]]), s = new Set(["x", "y"]);
var ki = m.keys();
var mi = m.entries();
var steps = [mi.next(), mi.next(), mi.next(), mi.next()];
var live = [];
var mm = new Map([[1, 1]]);
for (var [k] of mm) { live.push(k); if (k < 4) mm.set(k + 1, 0) }
var del = [];
var md = new Map([[1, 1], [2, 2], [3, 3]]);
for (var [k2] of md) { del.push(k2); if (k2 == 1) md.delete(2) }
var s1 = new Set([1]), d1 = s1.values();
d1.next(); d1.next(); s1.add(2);
var fe = [];
var sf = new Set([1]);
sf.forEach(function (v) { fe.push(v); if (v < 3) sf.add(v + 1) });
var errs = [];
try { ki.next.call({}) } catch (e) { errs.push(e.name) }
try { s.values().next.call(m.keys()) } catch (e) { errs.push(e.name) }
try { Map() } catch (e) { errs.push(e.name) }
try { Set() } catch (e) { errs.push(e.name) }
try { Map.call(new Map()) } catch (e) { errs.push(e.name) }
var me = new Map([[1, [0]]]);
for (var e of me) e[1] = "changed";
var arr = [1], grown = [];
for (var x of arr) { grown.push(x); if (arr.length < 3) arr.push(x + 1) }
var result = JSON.stringify([Array.isArray(m.keys()), Object.prototype.toString.call(ki), Object.prototype.toString.call(s.values()), ki[Symbol.iterator]() === ki, Map.prototype[Symbol.iterator] === Map.prototype.entries, Set.prototype[Symbol.iterator] === Set.prototype.values, Set.prototype.keys === Set.prototype.values, Map.prototype[Symbol.iterator].name, Set.prototype.keys.name, Map.prototype.keys.length, steps, [...s.entries()], [...s.keys()], [...m.values()], live, del, d1.next(), fe, errs, me.get(1), grown, Object.prototype.toString.call([].keys()), Array.isArray([1].entries()), [...[5, 6].entries()], [...[5, 6].keys()], Array.from(new Map([[1, 2]]))]);
