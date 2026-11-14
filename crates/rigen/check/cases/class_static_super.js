// A static method's [[HomeObject]] is the class constructor, so its `super` is the parent
// class, not the parent's prototype (ECMA-262 §15.7.14 ClassDefinitionEvaluation, §10.2.7);
// the same for static accessors and generators (static fields: class_static_blocks.js).
class A {
  static sm() { return "A.sm" }
  static get sg() { return "get:" + this.name }
  static *gen() { yield "A.gen" }
  im() { return "A.im" }
}
class B extends A {
  static sm() { return super.sm() }
  static n() { return super.name }
  static *g2() { yield super.name }
  static get sg() { return super.sg }
  static noProto() { return super.im }
  im() { return super.im() + "/" + typeof super.sm }
}
class C { static m() { return super.hasOwnProperty === Object.prototype.hasOwnProperty && super.call === Function.prototype.call } }
var result = JSON.stringify([B.sm(), B.n(), B.g2().next().value, B.sg, B.noProto(), new B().im(),
  C.m()]);
// v4: ["A.sm","A","A","get:B",null,"A.im/undefined",true]
