// v4: F|F|F|F|F|R|R|R|J|J|H|H|5,6
// An own or inherited `toString` on an array or a RegExp is the one a call, `+`, a template,
// String() and join() run (EvaluateCall, ECMA-262 §13.3.6.1; OrdinaryToPrimitive, §7.1.1.1),
// and %Array.prototype.toString% calls `this.join` (§23.1.3.36).
var f = [1, 2];
f.toString = function () { return "F" };
var r = /x/;
r.toString = function () { return "R" };
var g = [3];
Object.defineProperty(g, "join", { value: function () { return "J" } });
var h = Object.setPrototypeOf([4], Object.create(Array.prototype, {
    toString: { value: function () { return "H" } }
}));
var result = [f.toString(), f + "", `${f}`, String(f), [f].join(), r.toString(), String(r), r + "",
    g.toString(), String(g), h.toString(), String(h), [5, 6].toString()].join("|");
