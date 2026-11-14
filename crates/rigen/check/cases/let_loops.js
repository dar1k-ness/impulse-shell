// Per-iteration `let`/`const` bindings in for, for-in and for-of loops.
// v4: [[0,1,2],["0:10","1:9","2:8"],[0,2,4],[1,3],[0,1,2,1,2,3],[0,1,2],["x","y","z"],["p","q"],[2,12],[15,18],[0,10,11,20,21,22],[3,3,3],[0,1],[0,1,2]]
var a = [];
for (let i = 0; i < 3; i++) a.push(() => i);
var b = [];
for (let i = 0, j = 10; i < 3; i++, j--) { b.push(function () { return i + ":" + j }) }
var c = [];
for (let i = 0; i < 5; i++) { if (i % 2) continue; c.push(() => i) }
var d = [];
for (let i = 0; i < 3; i++) { d.push(() => i); i++ }
var e = [];
for (let i = 0; i < 3; i++) e.push(() => i++);
var e1 = e.map(f => f());
var e2 = e1.concat(e.map(f => f()));
var f = [];
for (let i = 0; f.push(() => i), i < 2; i++);
var g = [];
for (const k in { x: 1, y: 2, z: 3 }) g.push(() => k);
var h = [];
for (let k in { p: 1, q: 2 }) h.push(() => k);
var m = [];
for (const [x, y] of [[1, 2], [3, 4]]) m.push(() => x * y);
var n = [];
for (let v of [5, 6]) { let w = v * 2; n.push(() => v + w) }
var o = [];
outer: for (let i = 0; i < 3; i++) for (let j = 0; j < 3; j++) { if (j > i) continue outer; o.push(() => i * 10 + j) }
var p = [];
for (var q = 0; q < 3; q++) p.push(() => q);
var r = [];
for (let i = 0; i < 2; i++) setTimeoutLike(() => r.push(i));
function setTimeoutLike(fn) { r.push(fn) }
var call = r.slice(); r.length = 0; call.forEach(fn => fn());
var s = (function () { var out = []; for (let i = 0; i < 3; i++) out.push(() => i); return out.map(z => z()) })();
var result = JSON.stringify([a.map(z => z()), b.map(z => z()), c.map(z => z()), d.map(z => z()), e2, f.map(z => z()), g.map(z => z()), h.map(z => z()), m.map(z => z()), n.map(z => z()), o.map(z => z()), p.map(z => z()), r, s]);
