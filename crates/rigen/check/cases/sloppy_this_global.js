// Sloppy code called without a receiver (or with undefined or null) binds `this` to the global
// object (ECMA-262 §10.2.1.2 OrdinaryCallBindThis): an extensible object inheriting
// Object.prototype whose built-ins are read-only, non-enumerable and non-configurable as in
// QV4. A free name no declaration binds reads its properties; QML code still cannot assign one.
// v4: ["object","false","true","true","true","object","true","true","true","true","0","[object Object]","true","true","false","false","false","undefined","true","Infinity","true","function","function","false","object","true","TypeError","number","5","Error","5","true","undefined"]
function f() { return this; }
var g = f();
var d = Object.getOwnPropertyDescriptor(g, 'Math');
var r = [
    typeof g, g === undefined, f.call(null) === g, f.call(undefined) === g,
    (function () { 'use strict'; return this; })() === undefined, typeof f.call(1),
    g.Math === Math, g.JSON === JSON, g.parseInt === parseInt, 'Array' in g,
    Object.keys(g).length, Object.prototype.toString.call(g),
    Object.getPrototypeOf(g) === Object.prototype, Object.isExtensible(g),
    d.writable, d.enumerable, d.configurable, g.undefined, isNaN(g.NaN), g.Infinity,
    (function* () { yield this; })().next().value === g, typeof toString, typeof hasOwnProperty,
    delete g.Math, typeof Math,
];
g.Math = 1;
r.push(Math === g.Math);
try { (function () { 'use strict'; g.Math = 1; })(); r.push('no'); } catch (e) { r.push(e.name); }
g.zz = 5;
r.push(typeof zz, zz);
try { zz = 6; r.push('no'); } catch (e) { r.push(e.name); }
r.push(g.zz, delete g.zz, typeof zz);
var result = JSON.stringify(r.map(String));
