// for-in skips a property deleted before the enumeration reaches it, on the object itself, on
// its prototype chain and in a generator (ECMA-262 §14.7.5.9 EnumerateObjectProperties,
// §14.7.5.10 CreateForInIterator), and a symbol primitive enumerates %Symbol.prototype%.
// v4: a,c|a,b,c|0,1|a,y|a,x|a,b|a,c|ss|true
function ks(o, step) { var a = []; for (var k in o) { a.push(k); step(o, k); } return a.join(); }
var p = { x: 1, y: 2 };
var q = Object.create(p);
q.a = 1;
var s = { x: 1 };
var shadow = Object.create(s);
shadow.a = 1;
shadow.x = 2;
function* gen(o) { for (var k in o) { delete o.b; yield k; } }
var g = [];
for (var k of gen({ a: 1, b: 2, c: 3 })) g.push(k);
Symbol.prototype.ss = 1;
var sym = [];
for (var k in Symbol("t")) sym.push(k);
delete Symbol.prototype.ss;
var result = [
  ks({ a: 1, b: 2, c: 3 }, function (o) { delete o.b; }),
  ks({ a: 1, b: 2, c: 3 }, function (o, k) { delete o[k]; }),
  ks([1, 2, 3], function (o) { delete o[2]; }),
  ks(q, function () { delete p.x; }),
  ks(shadow, function (o) { delete o.x; }),
  ks({ a: 1, b: 2 }, function (o) { o.a = 5; }),
  g.join(),
  sym.join(),
  Object.getPrototypeOf(Symbol()) === Symbol.prototype
].join("|");
