// Audit case, assorted: `arguments`, `this` of calls, tagged templates, getters and setters,
// strict mode, JSON, string and array built-ins. The pin is ECMA-262's result (Node 22);
// Qt 6.12's V4 differs in several items. This runtime follows V4 in four: two are in
// audit_v4_ways.js, two are here: a template literal converts with hint `default`
// (`valueOf` first; `ecma.qml` `H1` of `builtins_b1_qt_equiv`), and `push` on a frozen array
// stores the element without a TypeError (`frozen.qml` `f2`).
// expect: ["2:9,2","AundefinedA1",1,"object[object Arguments]true","undefined,false","2,3",2,0,"o","undefined","o","global","o",true,true,"x|y\\n|z:1,2:3true",true,"a1b1,2[object Object]null",27,406,"0,f0,f1,2,f2","0,10",13,5000,"RangeError","vo","vo","vo","vo","vo","ts","ts",2,"1",2,true,true,"s",5,true,true,"string","a","{\"a\":2}",1,"Symbol(d)","d",1,true,true,true,true,true,true,true,7,"1,2,3,a,b,1","{\"a\":1}",2,true,3,false,"2,,6","[1,null,3]",-1,true,"0,2",2,1,2,10,10,"object","number",false,false,"c1","c2","c3",30,20,10,1,false,"TypeError","TypeError",1,null,"v",null,"undefined",null,null,null,true,"abc2",3,"\\n1",2,119987,2,"1,2,1,false","1,2,3,false","1,,2,false",1]
var log = [];
function args1() { arguments[0] = 9; return arguments.length + ":" + Array.prototype.slice.call(arguments).join(); }
function args2(a, b) { arguments[1] = "B"; a = "A"; return a + b + arguments[0] + arguments.length; }
function args3(a) { var f = () => arguments[0]; return f(2); }
function args4() { return typeof arguments + Object.prototype.toString.call(arguments) + (arguments.callee === args4); }
function args5(a, b) { delete arguments[0]; a = 7; return arguments[0] + "," + (0 in arguments); }
function args6(...r) { return r.length + "," + (function () { return arguments.length; })(...r, 1); }
log.push(args1(1, 2), args2(1), args3(1), args4(), args5(1, 2), args6(1, 2), args2.length, args6.length);
var o = { name: "o", m() { return this.name; }, n() { return (() => this.name)(); }, f: function () { return function () { return this === undefined ? "undef" : this === o ? "o" : "global"; }(); } };
var m = o.m; log.push(o.m(), typeof m.call(undefined), o.n(), o.f(), (o.m)(), (0, o.m)() === undefined, (o.m = o.m)() === undefined);
function tag(s, ...v) { return s.raw.join("|") + ":" + v.join() + ":" + s.length + Object.isFrozen(s); }
function same() { return (x => x)`a${1}b`; } log.push(tag`x${1}y\n${2}z`, same() === same(), `a${1}${"b"}${[1, 2]}${{}}${null}`);
var cnt = 0, obj = { get v() { return ++cnt; }, set v(x) { cnt += x; } }; obj.v += 10; obj.v++; log.push(cnt);
var s = 0; for (var i = 0; i < 5; i++) { switch (i) { case 1: continue; case 3: break; default: s += i; } s += 100; } log.push(s);
var fs = []; for (let i = 0; i < 3; i++) { try { if (i == 1) continue; fs.push(() => i); } finally { fs.push(() => "f" + i); } } log.push(fs.map(f => f()).join());
outer: for (let a = 0; a < 3; a++) { for (let b = 0; b < 3; b++) { if (b == 1) continue outer; if (a == 2) break outer; fs.push(() => a * 10 + b); } } log.push(fs.slice(5).map(f => f()).join());
var w = 0; a: { b: { c: for (; ;) { w++; if (w > 2) break b; continue c; } w = 100; } w += 10; break a; w = 1000; } log.push(w);
function rec(n) { return n == 0 ? 0 : 1 + rec(n - 1); } try { log.push(rec(5000)); } catch (e) { log.push(e.name); }
try { (function inf() { inf(); })(); } catch (e) { log.push(e.name); }
var k1 = { valueOf() { log.push("vo"); return 1; }, toString() { log.push("ts"); return "s"; } }; log.push(k1 + 1, `${k1}`, k1 * 2, k1 == 1, k1 < 2, [k1] + "", ({ s: 5 })[k1]);
var date = new Date(2020, 0, 15, 10, 30); log.push(date + 1 === date.toString() + "1", date - 0 === date.getTime(), typeof (date + date));
var sym = Symbol("d"), so = { [sym]: 1, a: 2 }; log.push(Object.keys(so).join(), JSON.stringify(so), so[sym], sym.toString(), sym.description, Object.getOwnPropertySymbols(so).length);
var chain = { a: { b() { return this === chain.a; } } }; log.push(chain.a.b(), chain.a?.b(), chain?.a.b(), chain.a["b"](), (chain.a.b)(), chain.x?.b() === undefined, chain.a.c?.() === undefined);
var sp = [1, 2, 3], spo = { a: 1 }; log.push(Math.max(...sp, ...[7]), [...sp, ..."ab", ...new Set([1, 1])].join(), JSON.stringify(spo), [...[, 1]].length, 0 in [...[, 1]]);
var holes = [1, , 3]; log.push(holes.length, 1 in holes, holes.map(x => x * 2).join(), JSON.stringify(holes), holes.indexOf(undefined), holes.includes(undefined), Object.keys(holes).join(), [, ,].length, holes.forEach.length);
var cl = (function () { var c = 0; return { inc: function () { return ++c; }, get c() { return c; } }; })(); cl.inc(); cl.inc(); log.push(cl.c);
var lz = 0; var cond = false && lz++; cond = true || lz++; cond = null ?? (lz += 10); cond = 1 ? lz : lz++; log.push(lz, cond);
log.push((function () { return typeof this; }).call(5), (function () { "use strict"; return typeof this; }).call(5), (function () { return this; }).call(null) === undefined, (() => this) === undefined);
var comma = (log.push("c1"), log.push("c2"), "c3"); log.push(comma);
var x = 10; { let x = 20; { let x = 30; log.push(x); } log.push(x); } log.push(x);
var getset = Object.defineProperty({}, "p", { get() { return 1; }, configurable: false }); getset.p = 5; log.push(getset.p, delete getset.p);
(function () { "use strict"; try { getset.p = 5; } catch (e) { log.push(e.name); } try { Object.freeze([1]).push(2); } catch (e) { log.push(e.name); } try { delete getset.p; } catch (e) { log.push(e.name); } })();
var fr = Object.freeze({ a: 1 }); fr.a = 2; fr.b = 3; log.push(fr.a, fr.b);
log.push(void log.push("v"), typeof void 0, (function () { })(), (function () { return; })(), (() => { })(), (() => ({}))() instanceof Object);
var nestedTpl = `a${`b${`c${1 + 1}`}`}`; log.push(nestedTpl, `\u0041\x41\n`.length, String.raw`\n${1}`, "\u{1F600}".length, "𝒳".codePointAt(0), [..."𝒳y"].length);
function defaults(a, b = a * 2, c = arguments.length, d = this) { return [a, b, c, d === undefined].join(); } log.push(defaults(1), defaults(1, undefined, 3), defaults(1, null), defaults.length);
var result = JSON.stringify(log);
