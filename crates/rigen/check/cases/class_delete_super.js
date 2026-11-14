// `delete super.x` and `delete super[k]` (ECMA-262 §13.5.1.2 step 5.b) evaluate the Super
// Reference (GetThisBinding, then the key) and throw a ReferenceError, in strict and sloppy
// code alike; the property is not deleted. Before `super()` the this binding throws first,
// so the key is not evaluated (V8 evaluates it first; this follows ECMA-262). Qt 6.12's V4
// crashes on `delete super.x`.
var log = [];
function t(name, f) { try { log.push(name + ":" + f()) } catch (e) { log.push(name + ":" + e.name) } }
class A { x() {} }
class B extends A {
  constructor(early) { if (early) delete super[(log.push("early key"), "x")]; super() }
  m() { return delete super.x }
  n() { return delete super[(log.push("key"), "x")] }
  arrow() { return (() => delete super.x)() }
  static s() { return delete super.x }
}
t("member", function () { return new B().m() });
t("index", function () { return new B().n() });
t("arrow", function () { return new B().arrow() });
t("static", function () { return B.s() });
t("kept", function () { return typeof A.prototype.x });
t("before super()", function () { return new B(true) });
var p = { y: 1 };
var lit = { d() { return delete super.y } };
Object.setPrototypeOf(lit, p);
t("sloppy literal", function () { return lit.d() });
t("literal kept", function () { return p.y });
var result = JSON.stringify(log);
