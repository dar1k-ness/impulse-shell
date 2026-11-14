// Array callback methods read each element when they reach it: a callback that writes,
// pops or shrinks the array changes what later steps see (the length is read once).
// v4: [[1,2,30,1,2],true,false,20,1,3,[2,4,null],3,false,[5,3],4]
var log = [];
var a = [1, 2, 3];
a.forEach(function (x, i) { log.push(x); if (i == 0) a[2] = 30 });
var b = [1, 2, 3];
b.forEach(function (x) { log.push(x); b.pop() });
var c = [1, 2, 3];
var ev = c.every(function (x) { c.length = 1; return x == 1 });
var d = [1, 2, 3];
var sm = d.some(function (x) { d.length = 1; return x == 3 });
var e = [1, 2, 3];
var fd = e.find(function (x, i) { if (i == 0) e[1] = 20; return x == 20 });
var g = [1, 2, 3];
var fi = g.findIndex(function (x) { g.length = 1; return x === undefined });
var h = [1, 2, 3];
var rd = h.reduce(function (s, x) { h.pop(); return s + x });
var m = [1, 2, 3];
var mp = m.map(function (x) { m.length = 2; return x * 2 });
var fl = [1, 2, 3];
var ft = fl.filter(function (x, i) { if (i == 0) fl[1] = 5; return x > 2 });
var rr = [1, 2, 3];
var rdr = rr.reduceRight(function (s, x) { rr.length = 1; return s + x }, 0);
var result = JSON.stringify([log, ev, sm, fd, fi, rd, mp, mp.length, 2 in mp, ft, rdr]);
