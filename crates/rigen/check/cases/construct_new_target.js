// v4: ["new Math.max THROW TypeError","new parseInt THROW TypeError","new method THROW TypeError","new arrow THROW TypeError","R.construct non-ctor THROW TypeError","R.construct newTarget arrow THROW TypeError","R.construct newTarget non-ctor THROW TypeError","R.construct ctors => ;true","Array.of parseInt => [1,2]","Array.of Math.max => [1,2]","Array.from Math.max => true","newTarget proto => true","newTarget proto own => true","newTarget class => true","newTarget Array => true,true,3","newTarget Map => true,false","newTarget no proto => true","bound new => true","proxy no trap newTarget => true","proxy trap newTarget => true","proxy trap own => true","proxy non-ctor THROW TypeError","new Symbol THROW TypeError","R.construct Symbol THROW TypeError","extends null THROW TypeError","extends null proto => true","extends null ctor THROW TypeError","extends null return => true","extends Math.max THROW TypeError","extends arrow THROW TypeError","extends bad proto THROW TypeError","extends null proto ok => true","extends bound THROW TypeError","extends proxy => 1,true"]
// IsConstructor (ECMA-262 §7.2.4) and NewTarget in [[Construct]]: built-ins that are no
// constructors reject `new` and Reflect.construct (§10.3), Reflect.construct, bound functions
// and proxies make the object with NewTarget's prototype (§28.1.2, §10.4.1.2, §10.5.13), and a
// class extending null or a non-constructor throws (§15.7.14, §13.3.7.1). Each case runs under
// `t`, which records its result or what it threw.
var r = [];
function t(name, f) {
    try {
        r.push(name + " => " + f());
    } catch (e) {
        r.push(name + " THROW " + e.name);
    }
}
function F() { this.p = Object.getPrototypeOf(this) }
function G() {}
t("new Math.max", function() { return new Math.max() })
t("new parseInt", function() { return new parseInt("1") })
t("new method", function() { return new [].push() })
t("new arrow", function() { return new (() => 1)() })
t("R.construct non-ctor", function() { return Reflect.construct(Math.max, []) })
t("R.construct newTarget arrow", function() { return Reflect.construct(F, [], () => 1) })
t("R.construct newTarget non-ctor", function() { return Reflect.construct(F, [], Math.max) })
t("R.construct ctors", function() { return [Array, Object, Date, RegExp, Map, Set, WeakMap, WeakSet, Error, TypeError, Function, Number, String, Boolean, ArrayBuffer, Uint8Array].filter(c => !(Reflect.construct(c, [], G) instanceof G)).map(c => c.name).join() + ";" + (Reflect.construct(Promise, [function() {}], G) instanceof G) })
t("Array.of parseInt", function() { return JSON.stringify(Array.of.call(parseInt, 1, 2)) })
t("Array.of Math.max", function() { return JSON.stringify(Array.of.call(Math.max, 1, 2)) })
t("Array.from Math.max", function() { return Array.isArray(Array.from.call(Math.max, [1])) })
t("newTarget proto", function() { return Reflect.construct(F, [], G).p === G.prototype })
t("newTarget proto own", function() { var o = Reflect.construct(F, [], G); return Object.getPrototypeOf(o) === G.prototype && o.p === G.prototype })
t("newTarget class", function() { class A { constructor() { this.p = Object.getPrototypeOf(this) } } return Reflect.construct(A, [], G).p === G.prototype })
t("newTarget Array", function() { var a = Reflect.construct(Array, [3], G); return (a instanceof G) + "," + Array.isArray(a) + "," + a.length })
t("newTarget Map", function() { var m = Reflect.construct(Map, [], G); return (m instanceof G) + "," + Map.prototype.has.call(m, 1) })
t("newTarget no proto", function() { function H() {} H.prototype = 1; return Object.getPrototypeOf(Reflect.construct(F, [], H)) === Object.prototype })
t("bound new", function() { var B = F.bind(null); return new B().p === F.prototype })
t("proxy no trap newTarget", function() { var P = new Proxy(F, {}); return Reflect.construct(P, [], G).p === G.prototype })
t("proxy trap newTarget", function() { var seen; var P = new Proxy(F, { construct(tg, a, nt) { seen = nt; return {} } }); Reflect.construct(P, [], G); return seen === G })
t("proxy trap own", function() { var seen; var P = new Proxy(F, { construct(tg, a, nt) { seen = nt; return {} } }); new P(); return seen === P })
t("proxy non-ctor", function() { return new (new Proxy(Math.max, {}))() })
t("new Symbol", function() { return new Symbol() })
t("R.construct Symbol", function() { return Reflect.construct(Symbol, []) })
t("extends null", function() { class P extends null {} return typeof new P() })
t("extends null proto", function() { class P extends null {} return Object.getPrototypeOf(P.prototype) === null })
t("extends null ctor", function() { class P extends null { constructor() { super() } } return typeof new P() })
t("extends null return", function() { class P extends null { constructor() { return Object.create(P.prototype) } } return new P() instanceof P })
t("extends Math.max", function() { class P extends Math.max {} return typeof P })
t("extends arrow", function() { class P extends (() => 1) {} return typeof P })
t("extends bad proto", function() { function H() {} H.prototype = 1; class P extends H {} return typeof P })
t("extends null proto ok", function() { function H() {} H.prototype = null; class P extends H {} return Object.getPrototypeOf(P.prototype) === null })
t("extends bound", function() { function H() { this.h = 1 } var B = H.bind(null); class P extends B {} var o = new P(); return o.h + "," + (o instanceof P) })
t("extends proxy", function() { function H() { this.h = 1 } class P extends new Proxy(H, {}) {} var o = new P(); return o.h + "," + (o instanceof P) })
var result = JSON.stringify(r);
