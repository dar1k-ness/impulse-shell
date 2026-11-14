// Spec semantics where Qt 6.12's V4 deviates: `Reflect.construct(Map, entries, F)` makes the
// Map with F.prototype before AddEntriesFromIterable reads `set` (OrdinaryCreateFromConstructor,
// ECMA-262 §24.1.1.1 step 2, §24.2.2.1 step 2), so F.prototype's adder runs; V4 reads
// NewTarget's `prototype` after the constructor and calls %Map.prototype.set% instead.
var log = [];
function F() {}
F.prototype = Object.create(Map.prototype);
// Defined, not assigned: Map.prototype's `set` is read-only in a QML engine, and assignment
// does not shadow a read-only inherited property.
Object.defineProperty(F.prototype, "set", { value: function (k, v) { log.push("F" + k); return Map.prototype.set.call(this, k, v) } });
function G() {}
G.prototype = Object.create(Set.prototype);
Object.defineProperty(G.prototype, "add", { value: function (v) { log.push("G" + v); return Set.prototype.add.call(this, v) } });
var r = Reflect.construct(Map, [[[9, "c"]]], F);
var q = Reflect.construct(Set, [[5]], G);
var result = JSON.stringify([
    log.join(), Object.getPrototypeOf(r) === F.prototype, r.get(9),
    Object.getPrototypeOf(q) === G.prototype, q.has(5)
]);
