// JS semantics both backends once got wrong (V4/ECMAScript order and scoping).
// v4: [["[object Object]","k","2","[object Object]","k","3","[object Object]","2","2","m","7","m"],{"k":5,"n":{"k":10},"m":7},[11,20,31],1,120,"outer","function",2,1,4,1,"function",6]
var log = [];
function t(v) { log.push(String(v)); return v }
var o = { k: 1, n: { k: 5 } }, i = 0, arr = [10, 20, 30];
// The target's base and key run once, before the right-hand side.
t(o)[t("k")] = t(2);
t(o)[t("k")] += t(3);
t(o.n).k *= t(2);
arr[i++] += 1;
arr[t(2)]++;
o[t("m")] ??= t(7);
o[t("m")] ||= t(8);
// A named function expression sees its own name; the outer name is untouched.
var fact = "outer";
var f = function fact(n) { return n <= 1 ? 1 : n * fact(n - 1) };
var g = function self() { return typeof self };
// A class constructor's object result replaces `this`.
class A { constructor() { return { z: 2 } } }
class B { constructor() { this.y = 1; return 5 } }
class C extends B { constructor() { super(); this.x = 3 } }
function F() { return { z: 1 } }
// Function declarations of every case exist from the switch's start.
var sw;
switch (1) { case 1: sw = typeof h; break; case 2: function h() { return 4 } }
var swh;
switch (2) { case 1: function k2() { return 6 } case 2: swh = k2() }
var result = JSON.stringify([log, o, arr, i, f(5), fact, g(), new A().z, new B().y, new C().x + new C().y, new F().z, sw, swh]);
