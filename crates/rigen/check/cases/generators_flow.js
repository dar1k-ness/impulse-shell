// Generators: yield inside expressions keeps evaluation order, yield* forwards next/throw/
// return to an inner generator, for-of inside a generator closes what it iterates, and the
// %GeneratorPrototype% / %GeneratorFunction.prototype% chain (ECMA-262 §27.3, §27.5).
// v4: false:i1 | false:i2 | false:i3 | false:o1 | true:oret | true:undefined | true:R | true:undefined | true:ackm|aBc|{"k":"k","m":"V"} | ["or"] | false:and | false:1 | false:k | false:v | true:[{"n":11},["val"]] | [1] | [1,2,"1,2"] | true | function | true | true | object | true | false | true | true | true | false | TypeError | true:2 | true:undefined | Error:z | TypeError | 3 | [3,2,1] | [2,1] | true:x_y_AB | got A;inner caught E;inner fin;v=iret;inner fin;src closed;not closed early
function t(f) { try { return String(f()) } catch (e) { return e.name + (e instanceof TypeError ? "" : ":" + e.message) } }
function J(r) { return r.done + ":" + r.value }
var log = [];
function* inner() { try { var a = yield "i1"; log.push("got " + a); yield "i2" } catch (e) { log.push("inner caught " + e); yield "i3" } finally { log.push("inner fin") } return "iret" }
function* outer() { var v = yield* inner(); log.push("v=" + v); yield "o1"; return "oret" }
var o = outer(), r = [];
r.push(J(o.next()), J(o.next("A")), J(o.throw("E")), J(o.next()), J(o.next()), J(o.next()));
var o2 = outer(); o2.next(); r.push(J(o2.return("R")), J(o2.next()));
function* ord() { var s = ""; function f(x) { s += x; return x } var v = f("a") + (yield 1) + f("c"); var o = { k: f("k"), [f("m")]: yield 2 }; return s + "|" + v + "|" + JSON.stringify(o) }
var g1 = ord(); g1.next(); g1.next("B"); r.push(J(g1.next("V")));
function* logic(x) { var a = x && (yield "and"); var b = x || (yield "or"); var c = x ?? (yield "nul"); return [a, b, c].join() }
r.push(JSON.stringify([...logic(0)]), J(logic(5).next()));
function* assign() { var o = { n: 1 }; o.n += yield o.n; var a = [0]; a[yield "k"] = yield "v"; return JSON.stringify([o, a]) }
var g2 = assign(); r.push(J(g2.next()), J(g2.next(10)), J(g2.next(0)), J(g2.next("val")));
function* src() { try { yield 1; yield 2; yield 3 } finally { log.push("src closed") } }
function* user() { for (let v of src()) { if (v == 2) return "early " + v; yield v } }
r.push(JSON.stringify([...user()]));
function* fo() { var fs = []; for (const v of [1, 2]) { fs.push(() => v); yield v } yield fs.map(f => f()).join() }
r.push(JSON.stringify([...fo()]));
function* gfn() {}
var other = function* () {};
r.push(gfn.prototype.next === other.prototype.next, typeof gfn.prototype.next, Function.prototype.isPrototypeOf(gfn), gfn.call === Function.prototype.call, typeof gfn.prototype.constructor, gfn.prototype.constructor.prototype.next === gfn.prototype.next, gfn.prototype.hasOwnProperty("constructor"), gfn() instanceof gfn, Object.prototype.isPrototypeOf(gfn()), gfn.prototype.isPrototypeOf(gfn()), other.prototype.isPrototypeOf(gfn()));
var GPnext = gfn.prototype.next; gfn.prototype = 5; r.push(t(() => gfn() instanceof gfn));
var s1 = (function* () { yield 1 })(); r.push(J(s1.return(2)), J(s1.next()), t(() => s1.throw(new Error("z"))));
r.push(t(() => GPnext.call({})), t(() => { var [a, b] = (function* () { yield 1; log.push("not closed early"); yield 2; yield 3 })(); return a + b }));
function* rec(n) { if (n > 0) { yield n; yield* rec(n - 1) } }
r.push(JSON.stringify([...rec(3)]), JSON.stringify(Array.from(rec(2))));
function* tt() { function tag(s, a, b) { return s.raw.join("_") + a + b } return tag`x${yield 1}y${yield 2}` }
var g3 = tt(); g3.next(); g3.next("A"); r.push(J(g3.next("B")));
r.push(log.join(";"));
var result = r.join(" | ");
