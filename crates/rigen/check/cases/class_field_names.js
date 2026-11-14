// Class fields are DefineField steps (ECMA-262 §7.3.33): CreateDataPropertyOrThrow, with
// NamedEvaluation of an anonymous function or class initializer under a computed key, and
// every computed key evaluated once, at class definition. QV4 (Qt 6.12) has no class fields,
// so there is no v4 pin.
function r(f) { try { return String(f()) } catch (e) { return e.name } }
var k = "ck", n = 0;
function key() { n++; return "kk" }
var cases = [
  function () { class C { [k] = function () {} } return new C().ck.name },
  function () { class C { static [k] = () => 1 } return C.ck.name },
  function () { class C { [k + "c"] = class {} } return new C().ckc.name },
  function () { class C { [k] = function own() {} } return new C().ck.name },
  function () { class C { [1 + 1] = function () {} } return new C()[2].name },
  function () { class C { f = function () {} } return new C().f.name },
  function () { class C { static s = function () {} } return C.s.name },
  function () { n = 0; class C { [key()] = 1 } new C(); new C(); return n },
  function () { n = 0; class C { static [key()] = 1 } return n + ":" + C.kk },
  function () {
    var hit = 0;
    class B { set x(v) { hit++ } }
    class C extends B { x = 1 }
    var c = new C();
    return hit + ":" + c.hasOwnProperty("x");
  },
  function () {
    class C { [k] = 1 }
    var d = Object.getOwnPropertyDescriptor(new C(), k);
    return d.value + ":" + d.writable + d.enumerable + d.configurable;
  },
  function () { class C { static a = Object.preventExtensions(this); static b = 1 } return 1 },
  function () { class C { static name = "n"; static length = 5 } return C.name + C.length },
  function () {
    class B { get y() { return 0 } }
    class C extends B { y = 1 }
    return new C().y;
  },
  function () {
    var log = "";
    function key(s) { log += s; return s }
    class C { [key("a")] = 1; [key("b")]() {} static [key("c")] = 2; static [key("d")]() {} }
    return log;
  },
  function () {
    var i = 0;
    class C { [k + i++] = 1; static [k + i++] = 2; [k + i++] = 3 }
    var c = new C();
    return c.ck0 + ":" + C.ck1 + ":" + c.ck2 + ":" + i;
  },
];
var out = []; for (var i = 0; i < cases.length; i++) out.push(r(cases[i]));
var result = JSON.stringify(out);
