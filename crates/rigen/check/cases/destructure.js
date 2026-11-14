function f({ a, b: [x, , y = 7] = [], ...rest }, ...more) { return [a, x, y, rest, more] }
var [p, q = 2, ...r] = [1, undefined, 3, 4];
var { m, n: { k } } = { m: 1, n: { k: 2 } };
let s, t;
[s, t] = [t, 5];
var obj = {}; ({ a: obj.x, b: obj["y"] } = { a: 1, b: 2 });
var result = JSON.stringify([f({ a: 1, b: [2, 3], c: 4, d: 5 }, 6, 7), p, q, r, m, k, s, t, obj]);
