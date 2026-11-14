// OrdinaryOwnPropertyKeys (ECMA-262 §10.1.11.1): integer keys ascending, then string keys in
// creation order. A class's `length`, `name` and `prototype`, and its prototype's
// `constructor`, exist before any element (§15.7.14); the elements follow in source order
// whatever their kind, and a method named like an existing property keeps its place.
// Qt 6.12's V4 agrees except that it lists `prototype` before `length` and `name`; this
// follows ECMA-262.
class C {
  static b() {} m() {} static get g() { return 1 } n() {} static c() {}
  get h() { return 1 } static ["d"]() {} static 2() {} static 1() {} set h(v) {}
}
class N { static name() { return "n" } static length() {} static z() {} }
class R { x() {} y() {} x() {} }
var result = JSON.stringify([Object.getOwnPropertyNames(C), Object.getOwnPropertyNames(C.prototype),
  Object.getOwnPropertyNames(N), Object.getOwnPropertyNames(R.prototype)]);
