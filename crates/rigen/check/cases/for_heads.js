// v4: 2,2|1,2|a,b|2|5|p|6|outer|undefined|7,8|r
// for-in/of heads (ECMA-262 14.7.5): `var` is one function-scoped binding, `let`/`const` a fresh
// binding per iteration, a bare target is assigned.
function heads() {
  var out = [], fv = [], fl = [], fc = [];
  for (var v of [1, 2]) fv.push(() => v);
  for (let l of [1, 2]) fl.push(() => l);
  for (const k in { a: 1, b: 2 }) fc.push(() => k);
  out.push(fv.map(f => f()).join(), fl.map(f => f()).join(), fc.map(f => f()).join(), v);
  var t = 0, o = {};
  for (t of [3, 4]);
  for (o.k in { p: 1 });
  for ([t, o.j] of [[5, 6]]);
  out.push(t, o.k, o.j);
  let s = "outer";
  for (let s of [1]) {}
  for (var w in {}) {}
  out.push(s, typeof w);
  return out.join("|");
}
var gs = [];
for (let x of [7, 8]) gs.push(() => x);
for (var y in { q: 1, r: 2 });
var result = heads() + "|" + gs.map(g => g()).join() + "|" + y;
