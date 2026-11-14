// A computed class element key is evaluated in the class scope, outside the methods of the
// class being defined (ClassDefinitionEvaluation, ECMA-262 §15.7.14), so `super` in it, also
// inside an arrow, is the enclosing method's (its [[HomeObject]]'s prototype, §13.3.7.1).
// Qt 6.12's V4 agrees on the keys but lists a class's `prototype` before `length` and
// `name` (class_key_order.js); this follows ECMA-262.
var base = { k: "outer" };
var o = {
  m() {
    class C {
      [super.k]() { return 1 }
      static [super.k + "S"]() { return 2 }
      get [super.k + "G"]() { return 3 }
      static [(() => super.k + "A")()]() { return 4 }
    }
    return [Object.getOwnPropertyNames(C.prototype).join(), Object.getOwnPropertyNames(C).join()];
  }
};
Object.setPrototypeOf(o, base);
class Base { static get tag() { return "B" } tag2() { return "P" } }
class D extends Base {
  static make() { return class { static [super.tag + "x"]() {} } }
  make() { return class { [super.tag2() + "y"]() {} } }
}
var result = JSON.stringify([o.m(), Object.getOwnPropertyNames(D.make()).join(),
  Object.getOwnPropertyNames(new D().make().prototype).join()]);
