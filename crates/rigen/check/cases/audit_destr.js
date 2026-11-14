// Audit case, destructuring: defaults, rest, holes, iterator protocol, assignment targets,
// parameters. The pin is ECMA-262's result (Node 22); Qt 6.12's V4 differs in four items
// (parameter defaults of `f`, `d2 = d1`, `arguments.length` next to patterns, the swap
// `[arr2[1], arr2[0]] = arr2`).
// expect: ["t2",1,2,"4,5","t3",1,3,"{\"a\":1,\"b\":2,\"c\":3}",2,1,"1,2,3,,","5,6,,7,8","TypeError","TypeError","TypeError","next0","next1","ret",0,1,"next0","next1","next2","next3","next4","next5","0,1,2,3,4",5,"hi","tkey","tdef","def",3,7,9,1,7,true,"geta","geta",2,"td1","d1","d1",3,2,6,0,"deep",3,1,2,"1,1","fs","TypeError"]
var log = [];
function t(v) { log.push("t" + v); return v; }
var [a, , b = t(2), ...rest] = [1, 9, undefined, 4, 5];
log.push(a, b, rest.join());
var { x, y: { z = t(3) } = {} } = { x: 1, p: 2, q: 3 };
log.push(x, z);
var o = {}; [o.a, o["b"]] = [1, 2]; ({ k: o.c } = { k: 3, m: 4 }); log.push(JSON.stringify(o));
var s1 = 1, s2 = 2; [s1, s2] = [s2, s1]; log.push(s1, s2);
function f({ a = 1, b: [c, d] = [2, 3] } = {}, ...[e, g]) { return [a, c, d, e, g].join(); }
log.push(f(), f({ a: 5, b: [6] }, 7, 8));
try { var { nope } = null; } catch (e) { log.push(e.name); }
try { var [n1] = undefined; } catch (e) { log.push(e.name); }
try { var [n2] = {}; } catch (e) { log.push(e.name); }
var it = { [Symbol.iterator]() { var i = 0; return { next() { log.push("next" + i); return { value: i++, done: i > 5 }; }, return() { log.push("ret"); return {}; } }; } };
var [i1, i2] = it; log.push(i1, i2);
var [...all] = it; log.push(all.join());
var { length } = "hello"; log.push(length);
var [c1, c2] = "hi"; log.push(c1 + c2);
var { [t("key")]: computed = t("def") } = {}; log.push(computed);
for (var [k, v] of new Map([[1, 2], [3, 4]])) log.push(k + v);
for (var { p: pp = 9 } of [{}, { p: 1 }]) log.push(pp);
var q; var res = ([q] = [7]); log.push(q, Array.isArray(res));
var { a: aa, a: ab } = { get a() { log.push("geta"); return 1; } }; log.push(aa + ab);
var [d1 = t("d1"), d2 = d1] = []; log.push(d1, d2);
(function ({ w }, [u]) { log.push(w + u, arguments.length); })({ w: 1 }, [2]);
var fn = ({ a, b } = { a: 1, b: 2 }, [c] = [3]) => a + b + c; log.push(fn(), fn.length);
var nested = { l1: { l2: [{ l3: "deep" }] } }; var { l1: { l2: [{ l3 }] } } = nested; log.push(l3);
let sw1 = 1, sw2 = 2, sw3 = 3; [sw1, sw2, sw3] = [sw3, sw1, sw2]; log.push(sw1, sw2, sw3);
var arr2 = [1, 2]; [arr2[1], arr2[0]] = arr2; log.push(arr2.join());
var {0: first, [1]: second} = ["f", "s"]; log.push(first + second);
try { const [cc] = [1]; cc = 2; } catch (e) { log.push(e.name); }
var result = JSON.stringify(log);
