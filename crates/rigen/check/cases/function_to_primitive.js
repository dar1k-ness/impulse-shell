// A function converts to a primitive as any object does (OrdinaryToPrimitive, ECMA-262
// §7.1.1.1): its own `valueOf`, `toString` and `@@toPrimitive` are called.
// v4: [[43,84,true,true,42,-42],["X","X","X",1,"X",1,true],["default1",7,"string",1],["1","s",2,"s"],["T1","T"],"THROWS TypeError","THROWS TypeError",[6,true,"K!"],[4,true],"str1",0,[true,2,"xx",2,false]]
var out = [];
function t(fn) {
    try { out.push(fn()); } catch (e) { out.push("THROWS " + e.name); }
}
t(function () { function f() {} f.valueOf = function () { return 42; }; return [f + 1, f * 2, f < 50, f == 42, Number(f), -f]; });
t(function () { function f() {} f.toString = function () { return "X"; }; return ["" + f, `${f}`, String(f), ({ X: 1 })[f], [f].join(), "aXb".indexOf(f), f == "X"]; });
t(function () { function f() {} f[Symbol.toPrimitive] = function (h) { return h == "number" ? 7 : h; }; return [f + 1, +f, String(f), ({ string: 1 })[f]]; });
t(function () { function f() {} f.valueOf = function () { return 1; }; f.toString = function () { return "s"; }; return [f + "", String(f), f + 1, [f] + ""]; });
t(function () { var f = () => 1; f.valueOf = function () { return {}; }; f.toString = function () { return "T"; }; return [f + 1, `${f}`]; });
t(function () { var f = function () {}; f.valueOf = function () { return {}; }; f.toString = function () { return {}; }; return f + 1; });
t(function () { var f = function () {}; f[Symbol.toPrimitive] = function () { return {}; }; return `${f}`; });
t(function () { class K { static toString() { return "K!"; } static valueOf() { return 5; } } return [K + 1, K == 5, String(K)]; });
t(function () { var f = function () {}.bind(null); f.valueOf = function () { return 3; }; return [f + 1, f > 2]; });
t(function () { function f() {} f.valueOf = 5; f.toString = function () { return "str"; }; return f + 1; });
t(function () { function f() {} var o = {}; o[f] = 1; return Object.keys(o)[0].indexOf("function"); });
t(function () { function f() {} f.toString = function () { return "2"; }; return [new Date(2000, 0, 1) - f > 0, parseInt(f), "x".repeat(f), Math.max(f, 1), isNaN(f)]; });
var result = JSON.stringify(out);
