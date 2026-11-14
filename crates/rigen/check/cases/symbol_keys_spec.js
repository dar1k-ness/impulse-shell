// Symbol keys where Qt 6.12's V4 deviates from ECMA-262 and the runtime follows the spec:
// V4's Object.defineProperties/Object.create and Object.assign skip symbol keys (§20.1.2.3.1,
// §20.1.2.1), its proxy [[OwnPropertyKeys]] skips the duplicate and missing non-configurable
// key checks (§10.5.11), and Object.freeze of a proxy leaves the target's symbol properties
// alone (§7.3.15). Each case runs under `t`, which records its result or what it threw.
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
t("dps", function() { var p = {}; p[s] = {value: 7, enumerable: true}; var o = Object.defineProperties({}, p); return o[s] + d(o, s) })
t("create", function() { var p = {}; p[s] = {value: 8}; var o = Object.create(null, p); return o[s] + "," + Reflect.ownKeys(o).length })
t("assign", function() { var src = {a: 1}; src[s] = 2; var o = Object.assign({}, src); return o[s] + d(o, s) })
t("assign ro", function() { var o = {}; Object.defineProperty(o, s, {value: 1}); var src = {}; src[s] = 2; Object.assign(o, src); return o[s] })
t("proxy ownKeys dup", function() { var p = new Proxy({}, {ownKeys: function() { return [s, s] }}); return Reflect.ownKeys(p).length })
t("proxy ownKeys missing fixed", function() { var tg = {}; Object.defineProperty(tg, s, {value: 1}); var p = new Proxy(tg, {ownKeys: function() { return [] }}); return Reflect.ownKeys(p).length })
t("proxy freeze", function() { var tg = {}; tg[s] = 1; var p = new Proxy(tg, {}); Object.freeze(p); return Object.isFrozen(tg) + d(tg, s) })
var result = JSON.stringify(r);
