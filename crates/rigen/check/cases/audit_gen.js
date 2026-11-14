// Audit case, generators: `next` / `throw` / `return`, `yield*`, `finally` in a suspended
// body, arguments and `this` across yields.
// v4: ["x5","cE","fin","{\"value\":1,\"done\":false}","{\"value\":10,\"done\":false}","{\"value\":\"caught\",\"done\":false}","{\"value\":\"end\",\"done\":true}","{\"done\":true}","0,2","g4fin","rinner","1,2,a,b","g4fin","{\"value\":\"early\",\"done\":true}","{\"done\":true}","{\"value\":\"stopped\",\"done\":true}","{\"done\":true}","A","B",11,12,true,"{\"value\":\"cleanup\",\"done\":false}","{\"value\":\"R\",\"done\":true}","{\"done\":true}","v0,v1","{\"k\":\"v2\",\"v3\":5}","v4v5","object,2","1,si","before start","{\"done\":true}",3,"TypeError","0,1,1,2,3,5,8,13,21,34",1,"function","GeneratorFunction","[object Generator]","[object Generator]","object",true,"TypeError",15,"b",true]
var log = [];
function* g1(a) { var x = yield a; log.push("x" + x); try { var y = yield x * 2; log.push("y" + y); } catch (e) { log.push("c" + e); yield "caught"; } finally { log.push("fin"); } return "end"; }
var it = g1(1); log.push(JSON.stringify(it.next("ignored")), JSON.stringify(it.next(5)), JSON.stringify(it.throw("E")), JSON.stringify(it.next()), JSON.stringify(it.next()));
function* g2() { for (let i = 0; i < 3; i++) { if (i == 1) continue; yield () => i; } }
log.push(Array.from(g2()).map(f => f()).join());
function* g3() { var r = yield* g4(); log.push("r" + r); yield* "ab"; return 9; }
function* g4() { yield 1; try { yield 2; } finally { log.push("g4fin"); } return "inner"; }
log.push(Array.from(g3()).join());
var it3 = g3(); it3.next(); it3.next(); log.push(JSON.stringify(it3.return("early")), JSON.stringify(it3.next()));
function* g5() { var i = 0; while (true) { var cmd = yield i++; if (cmd === "stop") return "stopped"; } }
var it5 = g5(); it5.next(); it5.next(); log.push(JSON.stringify(it5.next("stop")), JSON.stringify(it5.next()));
function* g6() { switch (yield 1) { case "a": yield "A"; case "b": yield "B"; break; default: yield "D"; } label: for (var k of [1, 2, 3]) { for (var j of [10, 20]) { if (j == 20) continue label; if (k == 3) break label; yield k + j; } } }
var it6 = g6(); it6.next(); log.push(it6.next("a").value, it6.next().value, it6.next().value, it6.next().value, it6.next().done);
function* g7() { try { yield 1; } finally { yield "cleanup"; } }
var it7 = g7(); it7.next(); log.push(JSON.stringify(it7.return("R")), JSON.stringify(it7.next()), JSON.stringify(it7.next()));
function* g8() { var a = [yield 1, yield 2]; var o = { k: yield 3, [yield 4]: 5 }; log.push(a.join(), JSON.stringify(o)); return (yield 6) + (yield 7); }
var it8 = g8(), v8 = it8.next(), n8 = 0; while (!v8.done) { v8 = it8.next("v" + n8++); } log.push(v8.value);
function* g9() { yield this === undefined ? "undef" : typeof this; yield arguments.length; }
log.push(Array.from(g9.call(5, 1, 2)).join());
var gobj = { *gen() { yield 1; }, [Symbol.iterator]: function* () { yield "si"; } }; log.push([...gobj.gen(), ...gobj].join());
function* g10() { try { yield 1; } catch (e) { log.push("never"); } }
var it10 = g10(); try { it10.throw(new Error("before start")); } catch (e) { log.push(e.message); } log.push(JSON.stringify(it10.next()));
function* g11() { var x = 1; var f = () => x++; yield f; yield x; }
var it11 = g11(); var f11 = it11.next().value; f11(); f11(); log.push(it11.next().value);
try { var itr = g5(); itr.next(); (function* () { yield* itr; })().next(); var self = (function* () { yield self.next(); })(); self.next(); } catch (e) { log.push(e.name); }
function* fib() { var a = 0, b = 1; for (;;) { yield a; var t = a + b; a = b; b = t; } }
var fr = []; for (var n of fib()) { if (n > 50) break; fr.push(n); } log.push(fr.join());
var [d1, d2] = fib(); log.push(d1 + d2);
log.push(typeof g1, g1.constructor.name, Object.prototype.toString.call(g1()), g1().toString(), typeof g1.prototype, g1() instanceof g1);
try { new g1(); } catch (e) { log.push(e.name); }
function* g12() { const c = yield 1; { let c2 = c + (yield 2); yield () => c2; } }
var it12 = g12(); it12.next(); it12.next(10); log.push(it12.next(5).value());
function* g13() { do { var v = yield; if (v) continue; else break; } while (true); for (var p in { a: 1, b: 2 }) yield p; }
var it13 = g13(); it13.next(); it13.next(1); it13.next(0); log.push(it13.next().value, it13.next().done);
var result = JSON.stringify(log);
