// SuperCall binds `this` to the object the parent's [[Construct]] returns (ECMA-262
// §13.3.7.1, BindThisValue): an object returned by a base class or function constructor
// replaces the one made for NewTarget, through implicit constructors and several levels;
// `super()` evaluates to it. A derived constructor returning a primitive is a TypeError,
// one without `super()` or calling it twice a ReferenceError.
// v4: [true,false,false,true,3,true,true,1,true,true,"TypeError","ReferenceError","ReferenceError",true]
function t(f) { try { return f() } catch (e) { return e.name } }
var o = { tag: 1 };
class B { constructor() { return o } }
class C extends B {}
function F() { return o }
class G extends F { constructor() { super(); this.g = 3 } }
class H extends C { constructor() { var r = super(); this.h = r === o } }
class P { constructor() { this.a = 1; return 5 } }
class Q extends P {}
class R extends Q { constructor() { super(); return 5 } }
class S extends P { constructor() { super(); return undefined } }
class T extends P { constructor() {} }
class U extends P { constructor() { super(); super() } }
class V extends P { constructor() { return o } }
var c = new C(), g = new G(), h = new H(), q = new Q(), s = new S();
var result = JSON.stringify([c === o, c instanceof C, c instanceof B, g === o, o.g, h === o, o.h,
    q.a, q instanceof Q, s instanceof S, t(() => new R()), t(() => new T()), t(() => new U()),
    t(() => new V() === o)]);
