// v4: [2,[0,1,2],[10,20,30],3,120,[2,6],[1,2,3],6]
function counter() { let n = 0; return { inc: () => ++n, get: function () { return n } } }
var c1 = counter(); c1.inc(); c1.inc();
var fs = [];
for (let i = 0; i < 3; i++) fs.push(() => i);
var gs = [];
for (const x of [1, 2, 3]) gs.push(() => x * 10);
function outer() { var self = this; const f = () => arguments.length; return f() }
function rec(n) { return n <= 1 ? 1 : n * rec(n - 1) }
var mapped = [1, 2, 3].map(function (v, i) { return v * i }).filter(v => v > 0);
var sorted = [3, 1, 2].sort((a, b) => a - b);
let acc = 0; [1, 2, 3].forEach(v => { acc += v });
var result = JSON.stringify([c1.get(), fs.map(f => f()), gs.map(g => g()), outer(1, 2, 3), rec(5), mapped, sorted, acc]);
