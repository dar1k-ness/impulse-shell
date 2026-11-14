// A deliberate difference from Qt 6.12 (docs/qt-differences.md, "An array in V4's sparse
// storage"): V4 moves an array with an accessor element, or with a gap of 4096 or more, to
// a sparse storage whose cells are not in index order, and its `freeze`, `seal` and `sort`
// walk the cells by position. Here such an array follows ECMA-262.
// (crates/qml/tests/data/builtins_b1/arraystore2.here and arraystore.here hold these lines
// beside Qt's for the interpreter.)
// expect: ["q15 TypeError|1|1|0|0,length|{\"0\":{\"value\":1,\"writable\":false,\"enumerable\":true,\"configurable\":false},\"length\":{\"value\":1,\"writable\":false,\"enumerable\":false,\"configurable\":false}}","p1 7,2,3|3|0,1,2|0,1,2,length|{\"0\":{\"enumerable\":true,\"configurable\":false},\"1\":{\"value\":2,\"writable\":false,\"enumerable\":true,\"configurable\":false},\"2\":{\"value\":3,\"writable\":false,\"enumerable\":true,\"configurable\":false},\"length\":{\"value\":3,\"writable\":false,\"enumerable\":false,\"configurable\":false}}|true","p2 1,7,3|3|0,1,2|0,1,2,length|{\"0\":{\"value\":1,\"writable\":false,\"enumerable\":true,\"configurable\":false},\"1\":{\"enumerable\":true,\"configurable\":false},\"2\":{\"value\":3,\"writable\":false,\"enumerable\":true,\"configurable\":false},\"length\":{\"value\":3,\"writable\":false,\"enumerable\":false,\"configurable\":false}}|true","p3 1,2,7|3|0,1,2|0,1,2,length|{\"0\":{\"value\":1,\"writable\":true,\"enumerable\":true,\"configurable\":false},\"1\":{\"value\":2,\"writable\":true,\"enumerable\":true,\"configurable\":false},\"2\":{\"enumerable\":true,\"configurable\":false},\"length\":{\"value\":3,\"writable\":true,\"enumerable\":false,\"configurable\":false}}|true","p4 7,2,3|3|0,1,2|0,1,2,length|{\"0\":{\"enumerable\":true,\"configurable\":false},\"1\":{\"value\":2,\"writable\":true,\"enumerable\":true,\"configurable\":false},\"2\":{\"value\":3,\"writable\":true,\"enumerable\":true,\"configurable\":false},\"length\":{\"value\":3,\"writable\":true,\"enumerable\":false,\"configurable\":false}}|true","p5 7|1|0|0,length|{\"0\":{\"enumerable\":true,\"configurable\":false},\"length\":{\"value\":1,\"writable\":false,\"enumerable\":false,\"configurable\":false}}|true","p6 1,2,,,,7|6|0,1,5|0,1,5,length|{\"0\":{\"value\":1,\"writable\":false,\"enumerable\":true,\"configurable\":false},\"1\":{\"value\":2,\"writable\":false,\"enumerable\":true,\"configurable\":false},\"5\":{\"enumerable\":true,\"configurable\":false},\"length\":{\"value\":6,\"writable\":false,\"enumerable\":false,\"configurable\":false}}|true","p7 {\"1\":{\"value\":2,\"writable\":false,\"enumerable\":true,\"configurable\":false},\"5000\":{\"value\":1,\"writable\":false,\"enumerable\":true,\"configurable\":false},\"length\":{\"value\":5001,\"writable\":false,\"enumerable\":false,\"configurable\":false}}|true","p8 THROW TypeError: Cannot assign to read-only property \"1\"","s1 ok|2","s2 10002"]
function t(f) { var s; try { s = String(f()) } catch (e) { s = "THROW " + e.name + ": " + e.message } return s }
function d(a) { return [a, a.length, Object.keys(a), Object.getOwnPropertyNames(a), JSON.stringify(Object.getOwnPropertyDescriptors(a))].join("|") }
function w(a, i, v) { "use strict"; try { a[i] = v; return "ok" } catch (e) { return e.name } }
function del(a, i) { "use strict"; try { return delete a[i] } catch (e) { return e.name } }
function ro() { var a = [1]; Object.defineProperty(a, 0, {writable: false}); a.push(2); return a }
function g(a, i) { Object.defineProperty(a, i, {get: function() { return 7 }, configurable: true, enumerable: true}); return a }
var r = [];
r.push("q15 " + t(function() { var a = [1]; Object.defineProperty(a, 0, {get: function() { return 7 }, configurable: true}); Object.defineProperty(a, 0, {value: 1, writable: true}); Object.freeze(a); return w(a, 0, 2) + "|" + d(a) }));
r.push("p1 " + t(function() { var a = g([1, 2, 3], 0); Object.freeze(a); return d(a) + "|" + Object.isFrozen(a) }));
r.push("p2 " + t(function() { var a = g([1, 2, 3], 1); Object.freeze(a); return d(a) + "|" + Object.isFrozen(a) }));
r.push("p3 " + t(function() { var a = g([1, 2, 3], 2); Object.seal(a); return d(a) + "|" + Object.isSealed(a) }));
r.push("p4 " + t(function() { var a = g([1, 2, 3], 0); Object.seal(a); return d(a) + "|" + Object.isSealed(a) }));
r.push("p5 " + t(function() { var a = g([1], 0); Object.freeze(a); return d(a) + "|" + Object.isFrozen(a) }));
r.push("p6 " + t(function() { var a = g([1, 2], 5); Object.freeze(a); return d(a) + "|" + Object.isFrozen(a) }));
r.push("p7 " + t(function() { var a = []; a[5000] = 1; a[1] = 2; Object.freeze(a); return JSON.stringify(Object.getOwnPropertyDescriptors(a)) + "|" + Object.isFrozen(a) }));
r.push("p8 " + t(function() { var a = g([1, 2, 3], 1); a.push(4); a.sort(); return d(a) }));
r.push("s1 " + t(function() { var a = [1]; a[10000] = 1; Object.freeze(a); return w(a, 0, 2) + "|" + a[0] }));
r.push("s2 " + t(function() { "use strict"; var a = [1]; a[10000] = 1; Object.freeze(a); a.push(2); return a.length }));
var result = JSON.stringify(r);
