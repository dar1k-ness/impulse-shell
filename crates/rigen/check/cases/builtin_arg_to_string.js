// Built-ins convert their string arguments with ToString (ECMA-262 §7.1.17), running an
// object's own toString/valueOf/Symbol.toPrimitive, and arrays' elements likewise. QV4 6.12 has
// no Object.fromEntries and throws in replaceAll for a non-RegExp object pattern: both are in the
// Rust tests of qml builtins.rs instead.
// v4: ["\"ajj,1null\"","\"1,3,true\"","\"true,true\"","\"a|b|c,a,b\"","\"f0o|p,r\"","\"j\"","\"jjjqq1\"","2","true","\"jtrue\"","\"true,0\"","\"j0\"","1","\"j%2C%20jjj\"","throw RangeError","throw TypeError"]
function t(f) {
    try {
        var v = f();
        return typeof v === "string" ? JSON.stringify(v) : String(v);
    } catch (e) {
        return "throw " + e.name;
    }
}
var j = { toString: function () { return "j" } };
var n = { valueOf: function () { return 1 }, toString: null };
var calls = [];
var p = { toString: function () { calls.push("p"); return "o" } };
var r = { toString: function () { calls.push("r"); return "0" } };
var result = JSON.stringify([
    t(function () { return "a".concat(j, [j, n], null) }),
    t(function () { return "xjy".indexOf(j) + "," + "xjyj".lastIndexOf([j]) + "," + "jx".includes(j) }),
    t(function () { return "jx".startsWith(j) + "," + "xj".endsWith([j]) }),
    t(function () { return "ajbjc".split(j).join("|") + "," + "a1b".split(n) }),
    t(function () { return "foo".replace(p, r) + "|" + calls.join() }),
    t(function () { return "j".replace("j", function () { return j }) }),
    t(function () { return "q".padStart(4, [j]) + "q".padEnd(2, n) }),
    t(function () { return "\u0041\u030a".normalize({ toString: function () { return "NFD" } }).length }),
    t(function () { return "a".localeCompare([j]) < 0 }),
    t(function () { return new RegExp(j).source + new RegExp([j, "k"]).test("j,k") }),
    t(function () { return /j/.test(j) + "," + /j/.exec([j]).index }),
    t(function () { return "j".match(j)[0] + "j".search([j]) }),
    t(function () { return JSON.parse({ toString: function () { return '{"a":[1]}' } }).a[0] }),
    t(function () { return encodeURIComponent([j, " "]) + encodeURI(j) + decodeURIComponent([j]) + decodeURI(j) }),
    t(function () { return "x".concat({ toString: function () { throw new RangeError() } }) }),
    t(function () { return "x".indexOf({ toString: function () { return {} } }) }),
]);
