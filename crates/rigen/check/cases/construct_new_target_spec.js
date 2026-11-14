// IsConstructor and NewTarget in [[Construct]] where Qt 6.12's V4 deviates from ECMA-262 and
// the runtime follows the spec: V4's ArraySpeciesCreate accepts a non-constructor @@species
// (§10.4.2.3 step 7), its Object constructor ignores a NewTarget other than itself (§20.1.1.1 step 1),
// its [[Construct]] reads NewTarget's `prototype` after the body instead of before it
// (OrdinaryCreateFromConstructor, §10.1.13), its bound functions drop an explicit NewTarget
// (§10.4.1.2 step 5), and `new` of a proxy without a construct trap uses the target's own
// `prototype` rather than the proxy's (§10.5.13 step 7). Each case runs under `t`, which
// records its result or what it threw.
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
t("newTarget Object", function() { var o = Reflect.construct(Object, [5], G); return (o instanceof G) + "," + typeof o })
t("newTarget getter order", function() { var log = []; var nt = new Proxy(G, { get(tg, k) { log.push(String(k)); return tg[k] } }); function K() { log.push("body") } Reflect.construct(K, [], nt); return log.join() })
t("bound newTarget", function() { var B = F.bind(null); return Reflect.construct(B, [], G).p === G.prototype })
t("proxy no trap", function() { var P = new Proxy(F, {}); return new P().p === F.prototype })
t("species Math.max", function() { var a = []; a.constructor = {}; a.constructor[Symbol.species] = Math.max; return a.map(x => x) })
t("species arrow", function() { var a = []; a.constructor = {}; a.constructor[Symbol.species] = () => 1; return a.slice() })
var result = JSON.stringify(r);
