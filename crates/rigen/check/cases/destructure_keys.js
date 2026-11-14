var src = { a: 1, b: 2, c: 3, "d-e": 4, k1: 5 };
var { a, ...noA } = src;
var { b, "d-e": de, ...rest } = src;
var key = "k" + 1;
var { [key]: kv, c, ...dyn } = src;
var { ["a"]: aa, ...afterComputed } = src;
function g({ a: x, ...others }) { return [x, others] }
var t = 0;
class K {
  m() { return "m" }
  ["dyn" + "Name"]() { return "d" }
  static s() { return "s" }
  static ["st" + "Dyn"]() { return "sd" }
  get v() { return 7 }
}
var o = new K();
var result = JSON.stringify([a, noA, b, de, rest, kv, c, dyn, aa, afterComputed, g(src),
  o.m(), o.dynName(), K.s(), K.stDyn(), o.v, Object.keys(noA)]);
