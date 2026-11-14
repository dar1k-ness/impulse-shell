// `Function.prototype.apply` takes an array-like (CreateListFromArrayLike): any object with
// a length; a primitive throws a TypeError; null and undefined are no arguments.
// v4: ["a,b|2","|1","|0","|0","1,2|2","|0","TypeError","TypeError"]
function f() { return Array.prototype.slice.call(arguments).join(",") + "|" + arguments.length }
var r = [f.apply(null, { length: 2, 0: "a", 1: "b" }), f.apply(null, { length: 1 }), f.apply(null, null),
    f.apply(null, undefined), f.apply(null, [1, 2]), f.apply(null, {})];
try { f.apply(null, "ab"); r.push("no throw") } catch (e) { r.push(e.name) }
try { f.apply(null, 1); r.push("no throw") } catch (e) { r.push(e.name) }
var result = JSON.stringify(r);
