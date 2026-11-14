// A method call on undefined or null: V4's TypeError names the call (`Cannot call method`),
// also for a string key in brackets; a number key and `o.f?.()` name the read.
// v4: ["TypeError: Cannot call method 'toLowerCase' of undefined","TypeError: Cannot call method 'foo' of null","TypeError: Cannot call method 'push' of undefined","TypeError: Cannot read property 'name' of null","TypeError: Cannot read property 'x' of undefined","TypeError: Cannot call method 'foo' of undefined","TypeError: Cannot read property '0' of null",null,"TypeError: Cannot read property 'foo' of undefined","TypeError: Property 'nope' of object [object Object] is not a function","TypeError: Cannot call method 'toString' of null"]
function t(f) { try { return f(); } catch (e) { return e.name + ": " + e.message; } }
var u, n = null, o = { v: undefined, z: null };
var result = JSON.stringify([t(() => u.toLowerCase()), t(() => n.foo()), t(() => o.v.push(1)), t(() => o.z.name.length), t(() => u.x), t(() => u["foo"]()), t(() => n[0]()), t(() => o.v?.foo()), t(() => o.v.foo?.()), t(() => o.nope()), t(() => o.z.toString())]);
