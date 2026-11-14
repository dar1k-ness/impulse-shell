// Symbol-keyed properties through every ToPropertyKey path (ECMA-262 §7.1.19): property
// descriptors (§20.1.2), Reflect (§28.1), `in` and `delete`, integrity levels and proxies.
// Each case runs under `t`, which records its result or the name of what it threw.
// v4: ["dp => 1,{\"value\":1,\"writable\":false,\"enumerable\":false,\"configurable\":false},1,false","dp strict ro THROW TypeError","dp redefine THROW TypeError","dp same => 1","dp writable => 3{\"value\":3,\"writable\":true,\"enumerable\":true,\"configurable\":true}true","dp getter => 8,functionfunctionfalsetrue","dp getter only strict THROW TypeError","dp accessor to data => 5{\"value\":5,\"writable\":false,\"enumerable\":false,\"configurable\":true}","dp nonext THROW TypeError","dp keeps place => Symbol(a),Symbol(b)3","dps nonenum sym => false","gopd missing => undefined","gopd literal => {\"value\":1,\"writable\":true,\"enumerable\":true,\"configurable\":true}","gopds => {\"value\":2,\"writable\":true,\"enumerable\":true,\"configurable\":true}a","hasOwn => true,false,true","key toPrimitive => true,true,true,true,2","key sym object => 9","in prim THROW TypeError","delete => true,false","delete nonconf => false,1","delete nonconf strict THROW TypeError","freeze => 1,true{\"value\":1,\"writable\":false,\"enumerable\":true,\"configurable\":false}false","freeze strict THROW TypeError","seal => 2,truefalse{\"value\":2,\"writable\":true,\"enumerable\":true,\"configurable\":false}","isFrozen sym => false,false","nonext add => undefined,0","nonext add strict THROW TypeError","inherited ro => 1,false","assign nonenum => false","R.dp => true,false,1","R.gopd => {\"value\":3,\"writable\":true,\"enumerable\":true,\"configurable\":true}","R.has => true,false","R.get => 1,2","R.set => true,false,1","R.set recv => 5","R.set recv data => true,2,1","R.delete => true,false","R.ownKeys => 1,b,a,Symbol(s)","proxy traps => true,true,1|2,true,false|dp:symbol get:string gopd:symbol has:symbol get:symbol set:symbol gopd:symbol dp:symbol get:symbol del:symbol has:symbol","proxy ownKeys => a,Symbol(s),1,a","proxy forward => 4,4,1{\"value\":4,\"writable\":false,\"enumerable\":true,\"configurable\":false}"]
var r = [];
function t(name, f) {
    try {
        r.push(name + " => " + f());
    } catch (e) {
        r.push(name + " THROW " + e.name);
    }
}
function d(o, k) { return JSON.stringify(Object.getOwnPropertyDescriptor(o, k)) }
var s = Symbol("s");
t("dp", function() { var o = {}; Object.defineProperty(o, s, {value: 1}); o[s] = 2; return o[s] + "," + d(o, s) + "," + Object.getOwnPropertySymbols(o).length + "," + o.propertyIsEnumerable(s) })
t("dp strict ro", function() { "use strict"; var o = {}; Object.defineProperty(o, s, {value: 1}); o[s] = 2; return o[s] })
t("dp redefine", function() { var o = {}; Object.defineProperty(o, s, {value: 1}); Object.defineProperty(o, s, {value: 2}); return o[s] })
t("dp same", function() { var o = {}; Object.defineProperty(o, s, {value: 1}); Object.defineProperty(o, s, {value: 1}); return o[s] })
t("dp writable", function() { var o = {}; Object.defineProperty(o, s, {value: 1, writable: true, enumerable: true, configurable: true}); o[s] = 3; return o[s] + d(o, s) + o.propertyIsEnumerable(s) })
t("dp getter", function() { var o = {}; Object.defineProperty(o, s, {get: function() { return this.v * 2 }, set: function(x) { this.v = x }, configurable: true}); o[s] = 4; var g = Object.getOwnPropertyDescriptor(o, s); return o[s] + "," + typeof g.get + typeof g.set + g.enumerable + g.configurable })
t("dp getter only strict", function() { "use strict"; var o = {}; Object.defineProperty(o, s, {get: function() { return 1 }}); o[s] = 2; return o[s] })
t("dp accessor to data", function() { var o = {}; Object.defineProperty(o, s, {get: function() { return 1 }, configurable: true}); Object.defineProperty(o, s, {value: 5}); return o[s] + d(o, s) })
t("dp nonext", function() { var o = Object.preventExtensions({}); Object.defineProperty(o, s, {value: 1}); return o[s] })
t("dp keeps place", function() { var a = Symbol("a"), b = Symbol("b"); var o = {}; o[a] = 1; o[b] = 2; Object.defineProperty(o, a, {value: 3}); return Object.getOwnPropertySymbols(o).map(String).join() + o[a] })
t("dps nonenum sym", function() { var p = {}; Object.defineProperty(p, s, {value: {value: 1}}); var o = Object.defineProperties({}, p); return o.hasOwnProperty(s) })
t("gopd missing", function() { return Object.getOwnPropertyDescriptor({}, s) })
t("gopd literal", function() { var o = {[s]: 1}; return d(o, s) })
t("gopds", function() { var o = {a: 1}; o[s] = 2; var ds = Object.getOwnPropertyDescriptors(o); return JSON.stringify(ds[s]) + Object.keys(ds) })
t("hasOwn", function() { var o = {}; Object.defineProperty(o, s, {value: 1}); var c = Object.create(o); return o.hasOwnProperty(s) + "," + c.hasOwnProperty(s) + "," + (s in c) })
t("key toPrimitive", function() { var k = {toString: function() { return "x" }}; var w = {[Symbol.toPrimitive]: function() { return s }}; var o = {x: 1}; o[s] = 2; return o.hasOwnProperty(k) + "," + o.hasOwnProperty(w) + "," + (k in o) + "," + (w in o) + "," + Object.getOwnPropertyDescriptor(o, w).value })
t("key sym object", function() { var o = {}; Object.defineProperty(o, Object(s), {value: 9}); return o[s] })
t("in prim", function() { return s in 1 })
t("delete", function() { var o = {}; o[s] = 1; return (delete o[s]) + "," + (s in o) })
t("delete nonconf", function() { var o = {}; Object.defineProperty(o, s, {value: 1}); return (delete o[s]) + "," + o[s] })
t("delete nonconf strict", function() { "use strict"; var o = {}; Object.defineProperty(o, s, {value: 1}); return delete o[s] })
t("freeze", function() { var o = {}; o[s] = 1; Object.freeze(o); o[s] = 2; return o[s] + "," + Object.isFrozen(o) + d(o, s) + (delete o[s]) })
t("freeze strict", function() { "use strict"; var o = {}; o[s] = 1; Object.freeze(o); o[s] = 2; return o[s] })
t("seal", function() { var o = {}; o[s] = 1; Object.seal(o); o[s] = 2; return o[s] + "," + Object.isSealed(o) + Object.isFrozen(o) + d(o, s) })
t("isFrozen sym", function() { var o = {}; o[s] = 1; Object.preventExtensions(o); return Object.isFrozen(o) + "," + Object.isSealed(o) })
t("nonext add", function() { var o = Object.preventExtensions({}); o[s] = 1; return o[s] + "," + Object.getOwnPropertySymbols(o).length })
t("nonext add strict", function() { "use strict"; var o = Object.preventExtensions({}); o[s] = 1; return o[s] })
t("inherited ro", function() { var p = {}; Object.defineProperty(p, s, {value: 1}); var o = Object.create(p); o[s] = 2; return o[s] + "," + o.hasOwnProperty(s) })
t("assign nonenum", function() { var src = {}; Object.defineProperty(src, s, {value: 1}); return Object.assign({}, src).hasOwnProperty(s) })
t("R.dp", function() { var o = {}; return Reflect.defineProperty(o, s, {value: 1}) + "," + Reflect.defineProperty(o, s, {value: 2}) + "," + o[s] })
t("R.gopd", function() { var o = {[s]: 3}; return JSON.stringify(Reflect.getOwnPropertyDescriptor(o, s)) })
t("R.has", function() { var o = Object.create({[s]: 1}); return Reflect.has(o, s) + "," + Reflect.has({}, s) })
t("R.get", function() { var o = {get [s]() { return this.v }, v: 1}; return Reflect.get(o, s) + "," + Reflect.get(o, s, {v: 2}) })
t("R.set", function() { var o = {}; var ok = Reflect.set(o, s, 1); Object.defineProperty(o, s, {writable: false}); return ok + "," + Reflect.set(o, s, 2) + "," + o[s] })
t("R.set recv", function() { var o = {set [s](x) { this.v = x }}; var rc = {}; Reflect.set(o, s, 5, rc); return rc.v })
t("R.set recv data", function() { var o = {}; o[s] = 1; var rc = {}; return Reflect.set(o, s, 2, rc) + "," + rc[s] + "," + o[s] })
t("R.delete", function() { var o = {}; o[s] = 1; Object.defineProperty(o, Symbol.iterator, {value: 1}); return Reflect.deleteProperty(o, s) + "," + Reflect.deleteProperty(o, Symbol.iterator) })
t("R.ownKeys", function() { var o = {b: 1, 1: 2}; o[s] = 3; o.a = 4; return Reflect.ownKeys(o).map(String).join() })
t("proxy traps", function() {
    var log = [];
    var h = {
        defineProperty: function(tg, k, ds) { log.push("dp:" + typeof k); return Reflect.defineProperty(tg, k, ds) },
        getOwnPropertyDescriptor: function(tg, k) { log.push("gopd:" + typeof k); return Reflect.getOwnPropertyDescriptor(tg, k) },
        has: function(tg, k) { log.push("has:" + typeof k); return Reflect.has(tg, k) },
        get: function(tg, k, rc) { log.push("get:" + typeof k); return Reflect.get(tg, k, rc) },
        set: function(tg, k, v, rc) { log.push("set:" + typeof k); return Reflect.set(tg, k, v, rc) },
        deleteProperty: function(tg, k) { log.push("del:" + typeof k); return Reflect.deleteProperty(tg, k) }
    };
    var p = new Proxy({}, h);
    Object.defineProperty(p, s, {value: 1, configurable: true, writable: true});
    var a = p.hasOwnProperty(s) + "," + (s in p) + "," + p[s];
    p[s] = 2;
    var b = Reflect.get(p, s) + "," + (delete p[s]) + "," + (s in p);
    return a + "|" + b + "|" + log.join(" ")
})
t("proxy ownKeys", function() { var p = new Proxy({}, {ownKeys: function() { return ["a", s] }}); return Reflect.ownKeys(p).map(String).join() + "," + Object.getOwnPropertySymbols(p).length + "," + Object.getOwnPropertyNames(p).join() })
t("proxy forward", function() { var tg = {}; var p = new Proxy(tg, {}); Object.defineProperty(p, s, {value: 4, enumerable: true}); return tg[s] + "," + p[s] + "," + Reflect.ownKeys(p).length + d(p, s) })
var result = JSON.stringify(r);
