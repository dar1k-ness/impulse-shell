// `this` of the top-level code of a `.js` file a QML document imports is the global object in
// Qt 6.12, with or without `.pragma library`: the object a sloppy function called without a
// receiver gets, where `this.zz = 5` makes a global. The file's own declarations are not its
// properties.
// v4: ["object",true,true,"object",true,true,"object","undefined",true,"[object Object]","object","object","undefined","undefined","number",["number",6],false,true,false,true,"undefined","undefined"]
.pragma library
var g = this;
var top = typeof this;
function f() { return typeof this }
function fs() { "use strict"; return typeof this }
var o = { a: () => this === g, m() { return this === o }, s: this };
var arrow = () => typeof this;
function id() { return this }
var tries = [];
function t(f) { try { tries.push(f()) } catch (e) { tries.push(e.name) } }
t(() => top);
t(() => this === g);
t(() => g === Object(g));
t(() => arrow());
t(() => o.a());
t(() => o.m());
t(() => f());
t(() => fs());
t(() => id() === g);
t(() => String(g));
t(() => typeof g.Math);
t(() => typeof g.Qt);
t(() => typeof g.f);
t(() => typeof g.top);
t(() => { this.zz = 5; return typeof zz });
t(() => { g.yy = 6; return [typeof yy, g.yy] });
t(() => "f" in g);
t(() => "Math" in g);
t(() => g.hasOwnProperty("f"));
t(() => Object.getPrototypeOf(g) === Object.prototype);
t(() => (function () { "use strict"; return typeof this })());
t(() => typeof g.undefined);
var result = JSON.stringify(tries);
