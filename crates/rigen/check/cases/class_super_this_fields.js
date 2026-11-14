// Derived `this` after a base constructor returned an object: fields and writes go to that
// object; `this` (or `super.x`) before `super()` is a ReferenceError. An arrow function of a
// derived constructor is QV4's: it has no `this` and cannot call `super()`
// (`v4_derived_arrow.js`). QV4 6.12 has no class fields, so these run against the
// interpreter.
function t(f) { try { return f() } catch (e) { return e.name } }
var o = { tag: 1 }, seen;
class B { bf = 1; constructor() { seen = this; return o } }
class C extends B { cf = 2; constructor() { super(); this.g = 3 } }
class D extends B { df = 4 }
class E extends B { constructor() { var f = () => super(); f(); this.e = 5 } }
class K extends B { constructor() { var g = () => this; super(); this.k = g() === o } }
class L extends B { constructor() { this.x = 1; super() } }
class M extends B { constructor() { super.toString; super() } }
class N extends B { constructor() { var f = () => this; f(); super() } }
var c = new C(), sc = seen, d = new D(), e = t(() => new E()), k = t(() => new K());
var result = JSON.stringify([c === o, o.bf, o.cf, o.g, sc.bf, sc instanceof C, d === o, o.df,
    e === o, o.e, k === o, o.k, t(() => new L()), t(() => new M()), t(() => new N())]);
