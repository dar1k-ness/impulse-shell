// A refused [[Set]] or delete throws a TypeError in strict mode code (functions with
// "use strict", class bodies) and is ignored in sloppy code; a frozen array's elements and
// `length` are read-only as ECMA-262 says (QV4 leaves them writable: those entries are left out
// of the pin below, see strict_set_arrays.js).
function r(f) { try { return "ok:" + f() } catch (e) { return e.name } }
var cases = [
  function () { "use strict"; var o = Object.freeze({ a: 1 }); o.a = 2; return o.a },
  function () { "use strict"; var o = Object.freeze({ a: 1 }); o.b = 2; return o.b },
  function () { "use strict"; var o = Object.freeze({ a: 1 }); o["a"] = 2; return o.a },
  function () { "use strict"; var o = Object.freeze({ a: 1 }); delete o.a; return o.a },
  function () { "use strict"; var o = Object.freeze({ a: 1 }); var k = "a"; delete o[k]; return o.a },
  function () { "use strict"; var o = Object.create(Object.freeze({ a: 1 })); o.a = 2; return o.a },
  function () { "use strict"; var o = { get a() { return 1 } }; o.a = 2; return o.a },
  function () { "use strict"; var f = function (a) {}; f.length = 3; return f.length },
  function () { "use strict"; var s = "s"; s.x = 1; return s.x },
  function () { "use strict"; var n = 5; n.x = 1; return "x" },
  function () { "use strict"; var o = Object.preventExtensions({ a: 1 }); o.a = 2; o.b = 3; return o.a },
  function () { "use strict"; var v = 0; var o = Object.freeze({ set a(x) { v = x } }); o.a = 4; return v },
  function () { "use strict"; var o = Object.freeze({ a: 1 }); return (() => { { o.a = 2 } })() },
  function () { class K { m() { var o = Object.freeze({ a: 1 }); o.a = 2; return o.a } } return new K().m() },
  function () { class K { m() { var o = Object.freeze({ a: 1 }); delete o.a; return o.a } } return new K().m() },
  function () { var o = Object.freeze({ a: 1 }); o.a = 2; o.b = 3; return o.a + "," + o.b + "," + Object.isFrozen(o) },
  function () { var o = Object.freeze({ a: 1 }); var d = delete o.a; return d + "," + o.a },
  function () { var p = Object.freeze({ a: 1 }); var o = Object.create(p); o.a = 2; return o.a + "," + o.hasOwnProperty("a") },
  function () { var o = Object.freeze({ n: { a: 1 } }); o.n.a = 2; return o.n.a },
  function () { var o = { a: 1 }; return (Object.freeze(o) === o) + "," + Object.freeze(5) },
  function () { var o = Object.freeze({ a: 1, b: 2 }); return Object.keys(o).join() + JSON.stringify(o) },
  function () { var o = { get a() { return 1 } }; o.a = 2; return o.a },
  function () { var f = function (a) {}; f.length = 3; return f.length },
  function () { var s = "s"; s.x = 1; return typeof s.x },
  function () { var o = Object.preventExtensions({ a: 1 }); o.b = 1; return Object.isFrozen(o) + "," + ("b" in o) },
];
var result = JSON.stringify(cases.map(r));
// v4: ["TypeError","TypeError","TypeError","TypeError","TypeError","TypeError","TypeError","TypeError","TypeError","TypeError","TypeError","ok:4","TypeError","TypeError","TypeError","ok:1,undefined,true","ok:false,1","ok:1,false","ok:2","ok:true,5","ok:a,b{\"a\":1,\"b\":2}","ok:1","ok:1","ok:undefined","ok:false,false"]
