// Methods and arrows are no constructors and own no `prototype` (ECMA-262 §15.4.4 DefineMethod,
// §15.7.14); instanceof needs a callable with an object `prototype` (§7.3.21).
// v4: ["TypeError","undefined",false,"ok:object","object","TypeError","undefined","TypeError","undefined","TypeError","undefined","ok:object","object","TypeError","ok:false","TypeError","TypeError","ok:true","ok:true","ok:5,prototype"]
function t(f) { try { return "ok:" + f() } catch (e) { return e.name } }
var o = { m() { return 1 }, f: function () {}, a: () => 1 };
class C { constructor() {} m() {} static sm() {} }
function F() {}
var result = JSON.stringify([
    t(() => typeof new o.m()), typeof o.m.prototype, "prototype" in o.m,
    t(() => typeof new o.f()), typeof o.f.prototype, t(() => typeof new o.a()), typeof o.a.prototype,
    t(() => typeof new (new C().m)()), typeof C.prototype.m.prototype, t(() => typeof new C.sm()), typeof C.sm.prototype,
    t(() => typeof new C()), typeof C.prototype,
    t(() => ({} instanceof o.m)), t(() => 1 instanceof o.m), t(() => ({} instanceof o.a)), t(() => ({} instanceof {})),
    t(() => new F() instanceof F), t(() => new C() instanceof C),
    t(() => { o.m.prototype = 5; return o.m.prototype + "," + Object.keys(o.m) })]);
