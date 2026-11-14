// new.target and NewTarget (ECMA-262 §13.3.12, §9.4.5 GetNewTarget): `undefined` in a
// [[Call]], the [[Construct]]'s NewTarget otherwise (lexical in arrows); `super(...)` passes
// it on to the parent's [[Construct]] (§13.3.7.1), through a bound function (§10.4.1.2) and
// a proxy's `construct` trap (§10.5.13), and a derived class makes no object of its own
// (§10.2.2 step 6), so NewTarget's `prototype` is read once, by the base constructor.
var r = [];
function t(name, f) {
    try {
        r.push(name + " => " + f());
    } catch (e) {
        r.push(name + " THROW " + e.name);
    }
}
function F() { this.nt = new.target }
function G() {}
class A { constructor() { this.nt = new.target } }
function trapped(log) {
    return new Proxy(A, { construct(tg, a, nt) { log.push(nt); return Reflect.construct(tg, a, nt) } })
}
t("call", function() { function f() { return new.target } return f() === undefined })
t("method call", function() { var o = { m() { return new.target } }; return o.m() === undefined })
t("new", function() { return new F().nt === F })
t("reflect", function() { return Reflect.construct(F, [], G).nt === G })
t("arrow", function() { function H() { this.a = (() => new.target)() } return new H().a === H })
t("arrow call", function() { function H() { return (() => new.target)() } return H() === undefined })
t("nested function", function() { function H() { this.a = (function() { return new.target })() } return new H().a === undefined })
t("bound", function() { var B = F.bind(null); return new B().nt === F })
t("bound reflect", function() { var B = F.bind(null); return Reflect.construct(B, [], G).nt === G })
t("class", function() { return new A().nt === A })
t("class reflect", function() { return Reflect.construct(A, [], G).nt === G })
t("derived implicit", function() { class B extends A {} var o = new B(); return (o.nt === B) + "," + (o instanceof B) })
t("derived explicit", function() { class B extends A { constructor() { var before = new.target; super(); this.b = before } } var o = new B(); return (o.nt === B) + "," + (o.b === B) + "," + (o instanceof B) })
t("derived reflect", function() { class B extends A {} var o = Reflect.construct(B, [], G); return (o.nt === G) + "," + (Object.getPrototypeOf(o) === G.prototype) })
t("derived two levels", function() { class B extends A {} class C extends B { constructor() { super() } } var o = new C(); return (o.nt === C) + "," + (o instanceof C) })
t("derived function parent", function() { class B extends F {} return new B().nt === B })
t("super in arrow", function() { class B extends A { constructor() { var f = () => super(); f(); this.k = (() => new.target)() } } var o = new B(); return (o.nt === B) + "," + (o.k === B) })
t("super proxy trap", function() { var log = []; class B extends trapped(log) {} var o = new B(); return (log[0] === B) + "," + (o.nt === B) + "," + (o instanceof B) })
t("super proxy trap explicit", function() { var log = []; class B extends trapped(log) { constructor(x) { super(x); this.x = x } } var o = new B(7); return (log[0] === B) + "," + o.x + "," + (o instanceof B) })
t("super proxy trap reflect", function() { var log = []; class B extends trapped(log) {} var o = Reflect.construct(B, [], G); return (log[0] === G) + "," + (Object.getPrototypeOf(o) === G.prototype) })
t("super proxy trap args", function() { var seen; var P = new Proxy(A, { construct(tg, a, nt) { seen = a.length + ":" + a[0] + ":" + a[1]; return {} } }); class B extends P { constructor() { super(1, 2) } } new B(); return seen })
t("super proxy trap result", function() { var P = new Proxy(A, { construct(tg, a, nt) { return { z: 1 } } }); class B extends P { constructor() { super(); this.w = 2 } } var o = new B(); return o.z + "," + o.w + "," + (o instanceof B) })
t("super proxy no trap", function() { class B extends new Proxy(A, {}) {} var o = new B(); return (o.nt === B) + "," + (o instanceof B) })
t("super bound parent", function() { var BA = A.bind(null); BA.prototype = A.prototype; class B extends BA {} var o = new B(); return (o.nt === B) + "," + (o instanceof B) })
t("super twice", function() { var log = []; class B extends trapped(log) { constructor() { super(); super() } } try { new B() } catch (e) { return e.name + "," + log.length + "," + (log[1] === B) } })
t("prototype read once", function() { var gets = 0; var NT = new Proxy(function() {}, { get(tg, k) { if (k === "prototype") gets++; return tg[k] } }); class B extends A { constructor() { super() } } class C extends B {} Reflect.construct(C, [], NT); return gets })
t("super not constructor", function() { class B extends A { constructor() { super() } } Object.setPrototypeOf(B, Math.max); return typeof new B() })
var result = JSON.stringify(r);
