// ToPropertyKey order where Qt 6.12's V4 deviates from ECMA-262 and the runtime follows the
// spec: a computed literal key converts before its value is evaluated (ComputedPropertyName),
// a compound or update target's key converts once, GetValue storing it in the reference
// (§6.2.5.5; V4 and V8 convert it again for the write), and a nullish base throws before the
// key converts (ToObject comes first in GetValue).
var r = [];
function t(name, f) {
    try {
        r.push(name + " => " + f());
    } catch (e) {
        r.push(name + " THROW " + e.name);
    }
}
function key(log, s) { return {toString: function() { log.push("k"); return s }} }
var sym = Symbol("s");
t("literal order", function() { var log = []; var o = {[key(log, "a")]: (log.push("v"), 1)}; return log.join() + "," + o.a })
t("compound", function() { var log = []; var o = {a: 1}; o[key(log, "a")] += (log.push("v"), 1); return log.join() + "," + o.a })
t("update", function() { var log = []; var o = {a: 1}; o[key(log, "a")]++; return log.join() + "," + o.a })
t("null get", function() { var log = []; try { null[key(log, "a")] } catch (e) { return e.name + "," + log.length } })
t("null call", function() { var log = []; try { null[key(log, "a")]((log.push("arg"), 1)) } catch (e) { return e.name + "," + log.length } })
var result = JSON.stringify(r);
