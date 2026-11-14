// Class key order with fields (class_key_order.js): methods and accessors are defined during
// ClassElementEvaluation, static fields after all elements (ECMA-262 §15.7.14 step 31),
// instance fields on construction. Qt 6.12's V4 rejects class fields at parse time; this
// follows ECMA-262.
class F { static a = 1; static b() {} static c = 2; x = 1; m() {} static get g() { return 0 } y = 2 }
var f = new F();
f.z = 3;
var result = JSON.stringify([Object.getOwnPropertyNames(F), Object.getOwnPropertyNames(F.prototype),
  Object.getOwnPropertyNames(f)]);
