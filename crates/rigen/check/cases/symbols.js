// v4: [5,"symbol","Symbol(k)",null,["a"],5,1,"m",true,false,"[object T]",[0],0,0,1,"x",["y","z"],["r","r","r","TypeError","r","r","r"],1,true,false,7,"p:string","p:default",[0,1,2],[0,1,2],true,"a"]
var log = [];
var s = Symbol("k");
var o = { a: 1, [s]: 2, get [Symbol.toStringTag]() { return "T" } };
o[s] += 3;
var { [s]: picked } = o;
var c = Object.assign({}, o);
class K { [s]() { return "m" } static [Symbol.hasInstance](v) { return v === 1 } }
var it = { [Symbol.iterator]() { var i = 0; return { next() { return i < 3 ? { value: i++, done: false } : { done: true } }, return() { log.push("r"); return {} } } } };
var seen = [];
for (const x of it) { if (x === 1) break; seen.push(x) }
function first() { for (var x of it) return x }
var [p, q] = it;
var [h, ...tl] = "xyz";
try { for (var x of it) throw new TypeError("t") } catch (e) { log.push(e.name) }
outer: for (var y of [1, 2]) { for (var z of it) continue outer }
var wm = new WeakMap([[o, 1]]);
var ws = new WeakSet([o]);
var prim = { [Symbol.toPrimitive](h) { return h === "number" ? 7 : "p:" + h } };
var result = JSON.stringify([o[s], typeof s, String(s), Symbol.keyFor(s), Object.keys(o), picked, c.a, new K()[s](), 1 instanceof K, 2 instanceof K, Object.prototype.toString.call(o), seen, first(), p, q, h, tl, log, wm.get(o), ws.has(o), wm.has({}), +prim, String(prim), prim + "", Array.from(it), [...it], Symbol.for("a") === Symbol.for("a"), Symbol.keyFor(Symbol.for("a"))]);
