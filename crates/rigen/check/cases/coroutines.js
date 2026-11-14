// Async functions (ECMA-262 §15.8-§15.9) are not run as ordinary functions: neither engine
// implements them yet, so a call throws instead of returning the body's value (QV4 returns a
// promise), and `ir::unsupported` fails an AOT build. Generators (§15.5) run.
function t(f) { try { return "ran:" + f() } catch (e) { return e.name } }
async function a() { return 1 }
function* g() { yield 1 }
async function* ag() {}
var o = { async m() { return 1 }, *n() {} };
class C { async m() { return 1 } static *s() {} }
var result = JSON.stringify([
    t(a), t(g), t(ag), t(async () => 1), t(() => o.m()), t(() => o.n()),
    t(() => new C().m()), t(() => C.s()), typeof a, t(() => (function () { return 1 })())]);
