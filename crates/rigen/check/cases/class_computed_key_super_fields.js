// `super` in a computed field key is the enclosing method's, as for computed method keys
// (class_computed_key_super.js). Qt 6.12's V4 rejects class fields at parse time; this
// follows ECMA-262.
var base = { k: "outer" };
var o = {
  m() {
    class C { [super.k + "F"] = 1; static [super.k + "S"] = 2; static [(() => super.k + "A")()] = 3 }
    return [Object.keys(new C()).join(), Object.keys(C).join()];
  }
};
Object.setPrototypeOf(o, base);
var result = JSON.stringify(o.m());
