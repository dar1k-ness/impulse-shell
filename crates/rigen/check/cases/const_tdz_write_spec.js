// A write to a `const` in its temporal dead zone from a closure: ReferenceError (ECMA-262
// §9.1.1.1.5 SetMutableBinding, the binding is uninitialized), where Qt 6.12's V4 throws
// TypeError. Once the declaration ran, the same closure throws TypeError. A write to a class
// before its declaration: ReferenceError, where V4 does not throw.
// expect: ["ReferenceError","ReferenceError","ReferenceError","TypeError","ReferenceError"]
function kind(f) { try { f(); return "no throw" } catch (e) { return e.name } }
var r = [];
r.push(kind(function () { function w() { z = 5 } w(); const z = 1 }));
r.push(kind(function () { var w = () => { [z] = [5] }; w(); const z = 1 }));
r.push(kind(function () { function w() { z += 1 } w(); const z = 1 }));
r.push(kind(function () { function w() { z = 5 } const z = 1; w() }));
r.push(kind(function () { K = 1; class K {} }));
var result = JSON.stringify(r);
