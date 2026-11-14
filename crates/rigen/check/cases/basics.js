var a = 1, b = "x";
let c = [1, 2, 3];
const o = { p: 1, ["q" + a]: 2, ...{ r: 3 } };
function add(x, y = 10) { return x + y }
var t = `v=${a} ${b}${c.length}`;
var result = JSON.stringify([a + 2, b + a, add(1), add(1, 2), o, t, typeof a, typeof zz, void 0,
  -a, +"3", !a, ~5, 7 % 3, 2 ** 10, 1 < 2, "a" < "b", null ?? 4, 0 || "d", 1 && 2, a == "1", a === "1",
  5 >> 1, -5 >>> 28, 3 & 5, 3 | 4, 3 ^ 5, 1 << 3, "p" in o, c instanceof Array, (1, 2, 3)]);
