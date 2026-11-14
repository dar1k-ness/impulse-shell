// Array.prototype.join/toString/toLocaleString/sort convert elements with ToString
// (ECMA-262 §7.1.17): an element's own toString, valueOf (no toString) or Symbol.toPrimitive
// runs, also behind String(), `+`, templates and `==`; elements are read as the join reaches
// them, and Array.prototype.toString calls the receiver's `join`. QV4 6.12 ignores an own `join`
// in String(array) and overflows on cycles where ECMA-262/V8 join a cycle as "": those two stay
// out of this pinned case (Rust tests in qml builtins.rs).
// v4: ["\"j\"","\"q=1\"","\"j-j,---2\"","\"1j2\"","\"jjjj\"","true","\"5pstringpstring\"","throw TypeError","throw TypeError","\"1,x,z\"","\"k\"","\"[object Object]\"","\"j+5\"","\"a,b|true\"","throw RangeError","\"j,j,\"","\"j\"","\"1,2|j\""]
function t(f) {
    try {
        var v = f();
        return typeof v === "string" ? JSON.stringify(v) : String(v);
    } catch (e) {
        return "throw " + e.name;
    }
}
var j = { toString: function () { return "j" } };
var vo = Object.create(null);
vo.valueOf = function () { return 5 };
var tp = {};
tp[Symbol.toPrimitive] = function (h) { return "p" + h };
var m = [1, { toString: function () { m[2] = "z"; return "x" } }, 3];
var order = [];
var b = { toString: function () { order.push("b"); return "b" } };
var a = { toString: function () { order.push("a"); return "a" } };
var result = JSON.stringify([
    t(function () { return [j].join() }),
    t(function () { return [new URLSearchParams("q=1")].join() }),
    t(function () { return [j, [j, null], undefined, , 2].join("-") }),
    t(function () { return [1, 2].join(j) }),
    t(function () { return String([j]) + [j] + `${[j]}` + [j].toString() }),
    t(function () { return [j] == "j" }),
    t(function () { return [vo].join() + String([tp]) + [tp] }),
    t(function () { return [{ toString: function () { throw new TypeError("no") } }].join() }),
    t(function () { return [{ toString: function () { return {} } }].join() }),
    t(function () { return m.join() }),
    t(function () { return Array.prototype.toString.call({ join: function () { return "k" } }) }),
    t(function () { return Array.prototype.toString.call({ join: 1 }) }),
    t(function () { return Array.prototype.join.call({ length: 2, 0: j, 1: vo }, "+") }),
    t(function () { return [b, a].sort().join() + "|" + (order.length > 0) }),
    t(function () { return [{ toString: function () { throw new RangeError() } }, 1].sort() }),
    t(function () { return [j, [j], null].toLocaleString() }),
    t(function () { return [{ toLocaleString: function () { return j } }].toLocaleString() }),
    t(function () { return new Int8Array([1, 2]).toString() + "|" + Int8Array.prototype.toString.call([j]) }),
]);
