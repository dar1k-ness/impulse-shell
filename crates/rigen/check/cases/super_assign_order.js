// A super assignment evaluates GetThisBinding, the key expression and GetSuperBase before the
// right-hand side (ECMA-262 §13.3.7.1, §13.3.7.3), so a prototype swap in the right-hand side
// does not move the write; ToPropertyKey of a computed key runs after it (NOTE in §13.3.7.1).
// A compound assignment or update reads through super and writes to `this`, keeping the
// attributes of an own data property there (§10.1.9.2 step 2.d.iii). Sloppy code ignores a
// refused [[Set]]; an arrow uses the [[HomeObject]] of its method and, as Qt 6.12's V4, the `this`
// it was called with, so `super.ar = 1` in it does not reach the method's `this`. V4 reads the
// super base after the right-hand side, yields the old value from `super.n *= 3` and
// redefines the own property as enumerable, so no `// v4:` pin (eval.rs pins it).
var log = [];
var p1 = { set v(x) { log.push("p1 " + x) } };
var p2 = { set v(x) { log.push("p2 " + x) } };
Object.defineProperty(p2, "r", { value: 1, writable: false });
p2.x = 0;
p2.n = 4;
p2.e = 0;
var o = {
  m() { super.v = (Object.setPrototypeOf(o, p2), 1) },
  k() {
    var key = { toString() { log.push("key"); return "kk" } };
    super[key] = (log.push("rhs"), 2);
    return this.kk
  },
  ro() { super.r = 5; return this.hasOwnProperty("r") },
  px() { super.x = 3; return [this.hasOwnProperty("x"), this.x, p2.x] },
  arrow() { (() => { super.ar = 1 })(); return this.hasOwnProperty("ar") && !("ar" in p2) },
  comp() { var a = (super.n *= 3); var b = super.n++; return [a, b, this.n, p2.n] },
  keepAttrs() {
    Object.defineProperty(this, "e", { value: 1, writable: true, enumerable: false, configurable: true });
    super.e = 9;
    return [this.e, this.propertyIsEnumerable("e"), p2.e]
  },
  *gen() { super.g = yield 1; return this.hasOwnProperty("g") && !p2.hasOwnProperty("g") },
};
Object.setPrototypeOf(o, p1);
o.m();
var it = o.gen();
it.next();
var result = JSON.stringify([o.k(), o.ro(), o.px(), o.arrow(), o.comp(), o.keepAttrs(), it.next(6).value, o.g, log]);
