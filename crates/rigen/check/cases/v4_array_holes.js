// Array holes (ECMA-262 §10.4.2): elements that do not exist, unlike `undefined` ones.
// v4: ["|3","|3",false,false,true,"2|3","0,1|4",2,"0,1,2|4","1,3,,","0|2","1|2","1,2,4|5","0,1,3|4","0,2|3","0,1,3|4","0,1,2|3","1,2|3",0,3,3,[2,0],["1","3"],-1,0,true,-1,"0|1","-",[null,1]]
var log = [];
var a = Array(3);
var m = a.map(function (_, i) { return i });
var b = [1, , 3];
var c = [];
c[2] = "x";
var d = [1, 2];
d.length = 4;
var visits = 0;
[, 1, , 2].forEach(function () { visits++ });
var s = [3, , undefined, 1].sort();
var sp = [1, , 3].splice(0, 2);
var sl = [1, , 3, , 5].slice(1, 3);
var cc = [, 1].concat([2, , 3]);
var rv = [1, , 3, 4].reverse();
var sh = [, 1, , 2];
sh.shift();
var un = [1, , 2];
un.unshift(0);
var po = [1, 2, , 4];
po.pop();
po.pop();
po.push(5);
var f = Array(3).fill(7, 1);
var fi = [, 1].findIndex(function (x) { return x === undefined });
var r = [1, , 2].reduce(function (x, y, i) { log.push(i); return x + y });
var rr = [1, , 2].reduceRight(function (x, y, i) { log.push(i); return x + y });
var fk = [];
for (var k in [, "a", , "b"]) fk.push(k);
function keys(x) { return Object.keys(x).join(",") + "|" + x.length }
var result = JSON.stringify([keys(a), keys(m), 1 in b, b.hasOwnProperty(1), b.hasOwnProperty(0), keys(c), keys(d),
    visits, keys(s), String(s), keys(sp), keys(sl), keys(cc), keys(rv), keys(sh), keys(un), keys(po), keys(f), fi, r, rr,
    log, fk, [, 1].indexOf(undefined), [undefined, 1].indexOf(undefined), [, 1].includes(undefined),
    [1, , 3].lastIndexOf(undefined),
    keys([, 1].filter(function () { return true })), Array(2).join("-"), [, 1].map(function (x) { return x })]);
