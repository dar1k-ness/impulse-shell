// A class extending String, Number or Boolean constructs a wrapper with the built-in's
// internal slot ([[StringData]], [[NumberData]], [[BooleanData]]) and the derived class's
// prototype (OrdinaryCreateFromConstructor, ECMA-262 §22.1.1.1, §21.1.1.1, §20.3.1.1), through
// an implicit constructor and an explicit `super(...)` call alike.
class S extends String {}
class S2 extends String { constructor(v) { super(v + "!"); this.extra = 1 } shout() { return this.toUpperCase() } }
class N extends Number {}
class Bo extends Boolean {}
var s = new S("ab"), s2 = new S2("hi"), n = new N(2), bo = new Bo(false);
var result = JSON.stringify([s.length, s.toUpperCase(), n + 3, bo.valueOf(), s instanceof S,
  s instanceof String, typeof s, String(s), s2.shout(), s2.length, s2.extra, n.toFixed(1),
  Object.prototype.toString.call(n), bo.toString(), s[1], Object.keys(s)]);
// v4: [2,"AB",5,false,true,true,"object","ab","HI!",3,1,"2.0","[object Number]","false","b",["0","1"]]
