// Numeric builtins take ToNumber(ToPrimitive(x, number)) of their arguments (ECMA-262 §7.1.4):
// valueOf, then toString, a TypeError when both give objects, and exceptions propagate.
// v4: ["3","4","9","5","-3","NaN","abc","false","false","def","b","99","ff","1.50","2|3","5","0","1|2|3","10","1.5","Infinity","-Infinity","Infinity","boom","TypeError","7","1"]
function n(v) { return { valueOf: function () { return v; } }; }
var s = { valueOf: null, toString: function () { return "4"; } };
var log = [];
function t(k, v) { return { valueOf: function () { log.push(k); return v; } }; }
var r = [Math.abs(n(-3)), Math.floor(s), Math.pow(n(-3), 2), Math.hypot(n(3), 4), Math.min(n(-3), 0),
  Math.max(t("a", 1), t("b", NaN), t("c", 2)), log.join(""), isNaN(n(1)), isFinite(n(1 / 0)),
  "abcdef".slice(n(3)), "abc".charAt(n(1)), "abc".charCodeAt(n(2)), (255).toString(n(16)),
  (1.5).toFixed(n(2)), [1, 2, 3, 4].slice(n(1), n(-1)).join("|"), new Date(2020, n(5), 1).getMonth(),
  new Date(n(0)).getTime(), [3, 1, 2].sort(function (x, y) { return n(x - y); }).join("|"),
  parseInt({ toString: function () { return "12"; } }, n(8)),
  parseFloat({ toString: function () { return "1.5"; } }), 1 / Math.max(-0, 0), 1 / Math.min(0, -0),
  Math.hypot(NaN, -1 / 0)];
try { Math.abs({ valueOf: function () { throw new Error("boom"); } }); } catch (e) { r.push(e.message); }
try { Math.floor({ valueOf: function () { return {}; }, toString: function () { return {}; } }); }
catch (e) { r.push(e.name); }
var seen = [];
try {
  Math.max({ valueOf: function () { seen.push(1); throw 7; } }, { valueOf: function () { seen.push(2); return 0; } });
} catch (e) { r.push(e, seen.length); }
var result = JSON.stringify(r.map(String));
