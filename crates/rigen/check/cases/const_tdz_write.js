// A write to a `const` before its declaration ran throws ReferenceError, as a write to a `let`
// does: SetMutableBinding (ECMA-262 §9.1.1.1.5) checks that the binding is initialized before
// that it is mutable. Plain, compound and destructuring writes; an initialized `const` still
// throws TypeError. Through a closure and to a class, where V4 deviates: const_tdz_write_spec.js.
// v4: ["ReferenceError","ReferenceError","ReferenceError","ReferenceError","TypeError","ReferenceError","ReferenceError","ReferenceError","ReferenceError5"]
function kind(f) { try { f(); return "no throw" } catch (e) { return e.name } }
var r = [];
r.push(kind(function () { z = 5; const z = 1 }));
r.push(kind(function () { z += 5; const z = 1 }));
r.push(kind(function () { z++; const z = 1 }));
r.push(kind(function () { y = 5; let y = 1 }));
r.push(kind(function () { const z = 1; z = 2 }));
r.push(kind(function () { [z] = [5]; const z = 1 }));
r.push(kind(function () { ({ a: z } = { a: 5 }); const z = 1 }));
r.push(kind(function () { { z = 5; const z = 1 } }));
// The right-hand side is evaluated before the binding is checked.
r.push((function () { var s = 0; try { z = (s = 5); const z = 1 } catch (e) { return e.name + s } })());
var result = JSON.stringify(r);
