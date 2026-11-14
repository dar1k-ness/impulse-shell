// All parts of a class are strict mode code (ECMA-262 §11.2.2), in a sloppy enclosing function
// too: the heritage, computed keys and field initializers; a static field initializer runs with
// the class as `this`. QV4 (Qt 6.12) has no class fields, so there is no v4 pin.
function r(f) { try { return "ok:" + f() } catch (e) { return e.name } }
var cases = [
  function () { class K { static a = this } return K.a === K },
  function () { class K { static f = Object.freeze({}).x = 1 } return 1 },
  function () { class K { [Object.freeze({}).x = 1]() {} } return 1 },
  function () { class K extends (Object.freeze({}).x = 1, Object) {} return 1 },
  function () { class K { f = Object.freeze({}).x = 1 } new K(); return 1 },
  function () { class K { static f = delete Object.freeze({a:1}).a } return 1 },
  function () { class K {} Object.freeze({}).x = 1; return 1 },
];
var out = []; for (var i = 0; i < cases.length; i++) out.push(r(cases[i]));
var result = JSON.stringify(out);
