// `super` resolves through the method's [[HomeObject]] (ECMA-262 §10.2.7 MakeMethod,
// §13.3.7.3 GetSuperBase): an object literal's methods, accessors and generator methods have
// the literal, a class's methods its prototype. The base is the home object's prototype at
// the time of the access, and a getter found there gets `this` as its receiver. Arrows see the
// home object of their method; a borrowed method keeps its own. Qt 6.12's V4 gives object
// literal methods no home object (`super.g` is undefined there); this follows the spec.
function t(f) { try { return f() } catch (e) { return e.name } }
var p = { g() { return "p" + this.tag }, get x() { return "x" + this.tag }, k: 1 };
var s = Symbol("s");
p[s] = function () { return "sym" };
var o = {
  tag: "o",
  g() { return "o" + super.g() },
  get y() { return super.x },
  *gen() { yield super.g() },
  ["c" + 1]() { return super.g() },
  arrow() { return (() => super.g())() },
  idx() { return super["g"]() + super[s]() },
  nested() { return Object.setPrototypeOf({ m() { return super.v } }, { v: "inner" }).m() + super.k },
  plain: () => typeof this,
};
Object.setPrototypeOf(o, p);
var q = { m() { return super.k } };
var before = q.m();
Object.setPrototypeOf(q, { k: 2 });
var after = q.m();
var borrowed = o.g.call({ tag: "b" });
class A { m() { return "A" } get who() { return this.n } }
class B extends A { constructor() { super(); this.n = 5 } m() { return super.m() } w() { return super.who } }
var b = new B();
var bm = b.m();
Object.setPrototypeOf(B.prototype, { m() { return "X" }, get who() { return "X" + this.n } });
// A literal's non-method values keep the enclosing method's home object.
class Outer extends A {
  lit() {
    var l = { f: () => super.m(), v: super.m(), m() { return super.m() } };
    Object.setPrototypeOf(l, { m() { return "L" } });
    return [l.f(), l.v, l.m()];
  }
}
var outer = new Outer().lit();
var miss = { m() { return super.nope } };
var callMiss = { m() { return super.nope() } };
var result = JSON.stringify([o.g(), o.y, o.gen().next().value, o.c1(), o.arrow(), o.idx(),
  o.nested(), before, after, borrowed, bm, b.m(), b.w(), miss.m(), t(() => callMiss.m()), outer]);
