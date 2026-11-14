// Audit case, evaluation order of operands, keys, arguments and assignments. As in Qt 6.12's
// V4 (the pin), a method call on `null` and a call of an undeclared name evaluate their
// arguments before they throw; ECMA-262 throws first.
// v4: ["get a","5","set a 6",3,11,"12,2,3",2,1,"getq","3",1,"key","val","TypeError","arg","TypeError","arg2","ReferenceError",41,6,"aab","k1","k2","v","nn","and","and",1,"undefined","ReferenceError"]
var log = [];
function t(v) { log.push(String(v)); return v; }
var o = { get a() { log.push("get a"); return 1; }, set a(v) { log.push("set a " + v); } };
o.a += t(5);
var x = 1;
x += (x = 10, 2);
log.push(x);
var y = 1;
y = y + (y = 5) + y;
log.push(y);
var arr = [1, 2, 3], i = 0;
arr[i++] = arr[i++] + 10;
log.push(arr.join(), i);
var z = 0;
function side() { z = 100; return 1; }
z += side();
log.push(z);
var q = { v: 1 };
function getq() { log.push("getq"); return q; }
getq().v **= t(3);
log.push(q.v);
var n = null;
try { n[t("key")] = t("val"); } catch (e) { log.push(e.constructor.name); }
try { n.foo(t("arg")); } catch (e) { log.push(e.constructor.name); }
try { undefinedFn(t("arg2")); } catch (e) { log.push(e.constructor.name); }
var w = 5;
var r1 = w++ + w++ * --w;
log.push(r1, w);
var s = "a";
s += s += "b";
log.push(s);
var d = { };
d[t("k1")] = d[t("k2")] = t("v");
var u;
u = u ?? t("nn"); u = u || t("no"); u = u && t("and");
log.push(u);
var cnt = 0; var oo = { get p() { cnt++; return null; } };
oo.p; log.push(cnt);
log.push(typeof notDefined, (function(){ try { notDefined2; } catch (e) { return e.name; } })());
var result = JSON.stringify(log);
