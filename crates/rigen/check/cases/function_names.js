// Functions have `length` and `name` (ECMA-262 SetFunctionLength / SetFunctionName, names
// inferred where an anonymous definition is bound), RegExp has a real prototype whose
// methods are values, and Number.parseFloat / Number.parseInt are the global functions.
// v4: ["f","2","arrow","1","m","n","q r","C","s","t","1","E","named","1","1","assigned","dflt","fromDflt","p","","w","0","true","true","max","2","slice","2","0","2","true","true","false","RegExp","2","1","1","object","true","test","1","1","true","true","true","(?:)","","undefined","true","a","g","true","c","/x/g","/a/g","0","true","false","call","4"]
var f = function (a, b) {};
var arrow = (x) => x;
var o = {m: function () {}, n() {}, get g() { return 1; }, 'q r': () => 0};
var C = class { static s() {} t(a) {} };
var D = class E {};
function named(a, b = 1, c) {}
function rest(a, ...r) {}
var assigned;
assigned = function () {};
var [dflt = function () {}] = [];
var {fromDflt = () => 0} = {};
function param(p = function () {}) { return p.name; }
var re = /a/g;
var w = function () {};
w.name = 'changed';
w.length = 9;
var r = [
    f.name, f.length, arrow.name, arrow.length, o.m.name, o.n.name, o['q r'].name,
    C.name, C.s.name, new C().t.name, new C().t.length, D.name, named.name, named.length,
    rest.length, assigned.name, dflt.name, fromDflt.name, param(), (function () {}).name,
    w.name, w.length, f.hasOwnProperty('name'), f.hasOwnProperty('length'),
    Math.max.name, Math.max.length, [].slice.name, [].slice.length, ''.trim.length,
    parseInt.length, Number.parseFloat === parseFloat, Number.parseInt === parseInt,
    Number.isNaN === isNaN, RegExp.name, RegExp.length, Array.length, Object.length,
    typeof RegExp.prototype, re.test === RegExp.prototype.test, re.test.name, re.test.length,
    re.exec.length, re instanceof RegExp, re.constructor === RegExp,
    RegExp.prototype.constructor === RegExp, RegExp.prototype.source, RegExp.prototype.flags,
    RegExp.prototype.global, re.global, re.source, re.flags,
    RegExp.prototype.test.call(/b/, 'abc'), RegExp.prototype.exec.call(/b(c)/, 'abc')[1],
    RegExp.prototype.toString.call({source: 'x', flags: 'g'}), String(re),
    Object.keys(RegExp.prototype).length, re.hasOwnProperty('lastIndex'),
    re.hasOwnProperty('source'), Function.prototype.call.name, Date.prototype.setHours.length,
];
var result = JSON.stringify(r.map(String));
