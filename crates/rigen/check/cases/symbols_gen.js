// v4: [true,true,"[object Generator]","Generator","function",true,"Array Iterator","[object Array Iterator]","String Iterator",[1,2],[2,4],1,{"value":7,"done":true},{"done":true},[1,2,null],[1,"R"],["f","f","nundefined","nb","r7","tE","y9","closed","TypeError"]]
var log = [];
function* g() { yield 1; yield 2 }
function* gf() { try { yield 1; yield 2 } finally { log.push("f") } }
var gi = g();
for (var x of gf()) break;
var [p0] = gf();
var inner = { [Symbol.iterator]() { return this }, next(v) { log.push("n" + v); return { value: 1, done: false } }, return(v) { log.push("r" + v); return { value: v, done: true } } };
function* d() { yield* inner }
var dx = d(); dx.next("a"); dx.next("b");
var dret = dx.return(7);
var inner2 = { [Symbol.iterator]() { return this }, next() { return { value: 1, done: false } }, throw(e) { log.push("t" + e); return { value: 9, done: true } } };
function* d2() { var r = yield* inner2; log.push("y" + r) }
var d2x = d2(); d2x.next();
var d2r = d2x.throw("E");
var inner3 = { [Symbol.iterator]() { return this }, next() { return { value: 1, done: false } }, return() { log.push("closed"); return {} } };
function* d3() { yield* inner3 }
var d3x = d3(); d3x.next();
try { d3x.throw("E") } catch (e) { log.push(e.name) }
function* a() { var r = yield* [1, 2]; yield r }
function* i2() { yield 1; return "R" }
function* o2() { var r = yield* i2(); yield r }
var result = JSON.stringify([gi[Symbol.iterator]() === gi, gi[Symbol.iterator] === [][Symbol.iterator]()[Symbol.iterator], Object.prototype.toString.call(gi), gi[Symbol.toStringTag], typeof gi[Symbol.iterator], "next" in gi, [][Symbol.iterator]()[Symbol.toStringTag], Object.prototype.toString.call([].values()), ""[Symbol.iterator]()[Symbol.toStringTag], [...g()], Array.from(g(), x => x * 2), p0, dret, d2r, [...a()], [...o2()], log]);
