// A class constructor has [[Construct]] but its [[Call]] throws a TypeError (ECMA-262 §10.2.1
// step 2), also for a derived or implicit constructor, through call/apply and bound, while
// `new` and `super(...)` still run it.
// v4: ["TypeError","TypeError","TypeError","TypeError","TypeError","ok:1","ok:1","ok:3","ok:object"]
function t(f) { try { return "ok:" + f() } catch (e) { return e.name } }
class A { constructor() { this.x = 1 } }
class B extends A {}
class C {}
class D extends A { constructor() { super(); this.y = 2 } }
var result = JSON.stringify([
    t(() => A()), t(() => B.call({})), t(() => C.apply(null, [])), t(() => D.call({})),
    t(() => A.bind(null)()), t(() => new (A.bind(null))().x), t(() => new B().x),
    t(() => { var d = new D(); return d.x + d.y }), t(() => typeof new C())]);
