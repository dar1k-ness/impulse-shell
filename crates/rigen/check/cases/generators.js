// Generator functions (ECMA-262 §15.5, §27.5): suspension at yield, next/return/throw, finally
// on return, closing on for-of break and destructuring, spread, yield* delegation, methods.
// v4: false:1 | false:10 | false:fin | true:7 | true:undefined | function | TypeError | object | [object Generator] | [object Generator] | Error | true:4 | closed | closed | 0 | 1 | [8,null,"fin"] | TypeError | [1,2,4,null,"fin"] | 3 | 5 | 6 | g | 1 | m | object | [1,"caught in","f","after"] | false:caught out | false:f | false:after | true:undefined | [0,2,"d0","d1","a1"] | true:0,1,2 | true:7 | ["one","two","end"] | ["def","end"] | false:2 | 7
function t(f) { try { return String(f()) } catch (e) { return e.name } }
function J(r) { return r.done + ":" + r.value }
function* g(a) { var x = yield a; try { yield x * 2 } finally { yield "fin" } return 9 }
var it = g(1), r = [];
r.push(J(it.next()), J(it.next(5)), J(it.return(7)), J(it.next()), J(it.next()));
r.push(typeof g, t(() => new g()), typeof g.prototype, Object.prototype.toString.call(it), String(it));
r.push(t(() => { var i2 = g(0); return J(i2.throw(new Error("x"))) }), J(g(3).return(4)));
function* inf() { var i = 0; try { while (true) yield i++ } finally { r.push("closed") } }
for (var v of inf()) { if (v > 2) break }
var [p, q] = inf();
r.push(p, q, JSON.stringify([...g(8)]), t(() => { var s = (function* () { s.next() })(); s.next() }));
function* d() { var x = yield* [1, 2]; var y = yield* g(4); return y }
r.push(JSON.stringify([...d()]));
var o = { *m() { yield this.k }, k: 3 }; class C { *n() { yield 5 } static *s() { yield 6 } }
r.push(o.m().next().value, new C().n().next().value, C.s().next().value, g.name, g.length, o.m.name, typeof o.m.prototype);
function* cat() { try { yield 1; throw new Error("in") } catch (e) { yield "caught " + e.message } finally { yield "f" } yield "after" }
r.push(JSON.stringify([...cat()]));
var c2 = cat(); c2.next(); r.push(J(c2.throw(new Error("out"))), J(c2.next()), J(c2.next()), J(c2.next()));
function* loops(n) { for (let i = 0; i < n; i++) { if (i == 1) continue; yield i } var k = 0; do { yield "d" + k } while (++k < 2); lbl: for (var a of [1, 2, 3]) { for (;;) { if (a == 2) continue lbl; if (a == 3) break lbl; yield "a" + a; break } } }
r.push(JSON.stringify([...loops(3)]));
function* cl() { var fs = []; for (let i = 0; i < 3; i++) { fs.push(() => i); yield i } return fs.map(f => f()).join() }
var ci = cl(); ci.next(); ci.next(); ci.next(); r.push(J(ci.next()));
function* ex(a) { var s = a + (yield 1) * (yield 2); return s + (yield 3 ? "y" : "n") }
var e1 = ex(1); e1.next(); e1.next(2); e1.next(3); r.push(J(e1.next(0)));
function* sw(x) { switch (x) { case 1: yield "one"; case 2: yield "two"; break; default: yield "def" } yield "end" }
r.push(JSON.stringify([...sw(1)]), JSON.stringify([...sw(5)]));
function* args() { yield arguments.length; yield this }
r.push(J(args.call("T", 1, 2).next()), String(args.call("T").next() && [...args.call(7)][1]));
var result = r.join(" | ");
