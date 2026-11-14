// Spec over V4: object rest/spread (which V4 cannot parse) and Object.assign copy symbol-keyed
// properties (ECMA-262 §7.3.25, §20.1.2.1); a template literal converts with hint "string"
// (§13.15.5.1); Symbol.prototype.description (§20.4.3.2).
var s = Symbol("k"), t = Symbol("t");
var o = { a: 1, [s]: 2, [t]: 3 };
var { [s]: picked, ...rest } = o;
var { a, ...plain } = o;
var c = { ...o };
var as = Object.assign({}, o);
var prim = { [Symbol.toPrimitive](h) { return h } };
var result = JSON.stringify([s.description, `${prim}`, as[t], picked, rest[s], rest[t], plain[s], c[s], c[t], Object.getOwnPropertySymbols(rest).length]);
