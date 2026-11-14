// ClassStaticBlock (ECMA-262 §15.7.11): each block runs once, in order with the static field
// initializers, as a method of the class: `this` is the class, `super` its parent, `var`s stay
// inside the block. A static field initializer is also a method of the class, so its `super`
// is the parent class. Qt 6.12's V4 rejects static blocks and fields at parse time; this
// follows the spec.
var log = [];
class K {
  static a = log.push("a");
  static { log.push("block:" + this.a + ":" + (this === K)); var hidden = 1 }
  static b = log.push("b");
  static { log.push("last:" + this.b) }
  static name2() { return "K" }
}
class K2 extends K {
  static { log.push("super:" + super.name2() + ":" + (() => this === K2)()) }
  static f = super.name2();
  static arrow = (() => super.name2() + (this === K2))();
}
var result = JSON.stringify([log, typeof hidden, K2.f, K2.arrow]);
