// ToPrimitive of a function where QV4 (Qt 6.12) leaves ECMA-262, as it does for any object: a
// template literal converts as `+` does, with hint "default" (§13.2.8.6 says "string"), and
// an `@@toPrimitive` that is not callable is passed over (§7.1.1, GetMethod, says TypeError).
// v4: [["default","adefaultbdefault"],["1"],["5"],"function f() { [native code] }1"]
var out = [];
function t(fn) {
    try { out.push(fn()); } catch (e) { out.push("THROWS " + e.name); }
}
t(function () { function f() {} f[Symbol.toPrimitive] = function (h) { return h; }; return [`${f}`, `a${f}b${f}`]; });
t(function () { function f() {} f.valueOf = function () { return 1; }; f.toString = function () { return "s"; }; return [`${f}`]; });
t(function () { class K { static toString() { return "K!"; } static valueOf() { return 5; } } return [`${K}`]; });
t(function () { function f() {} f[Symbol.toPrimitive] = 5; return f + 1; });
var result = JSON.stringify(out);
