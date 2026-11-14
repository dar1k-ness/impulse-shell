// A frozen array's elements and `length` are read-only and the mutating methods, writing with
// Set(O, P, V, true), throw on it in any code (ECMA-262 §10.4.2, §23.1.3); QV4 leaves frozen
// arrays writable, so this case has no v4 pin.
function r(f) { try { return "ok:" + f() } catch (e) { return e.name } }
var cases = [
  function () { "use strict"; var a = Object.freeze([1]); a[0] = 2; return a[0] },
  function () { "use strict"; var a = Object.freeze([1]); a.length = 0; return a[0] },
  function () { "use strict"; var a = Object.preventExtensions([1]); a[1] = 2; return a.length },
  function () { var a = Object.freeze([1]); a[0] = 2; a[1] = 3; a.length = 0; var d = delete a[0]; return a + ":" + a.length + ":" + d },
  function () { var a = Object.freeze([1, 2]); a.push(3); return a.length },
  function () { var a = Object.freeze([1, 2]); a.sort(); return a + "" },
  function () { var a = Object.freeze([1]); return a.reverse() === a },
  function () { var a = Object.preventExtensions([1, 2]); a.pop(); a[0] = 7; return a + "" },
  function () { var a = Object.preventExtensions([1]); try { a.unshift(0) } catch (e) { return e.name + ":" + a } },
];
var result = JSON.stringify(cases.map(r));
