// Destructuring assignment evaluates a member target's reference (its base and key
// expressions) before the value is read: AssignmentElement before IteratorStepValue,
// KeyedDestructuringAssignmentEvaluation before the property Get, rest elements and
// properties before collecting the rest; then the default, then PutValue, which converts
// the key (ECMA-262 §13.15.5.3-6). Qt 6.12's V4 steps array patterns before evaluating
// their targets and rejects `...`, so this case has no `// v4:` pin.
var r = [];
function t(name, f) {
    try {
        r.push(name + " => " + f());
    } catch (e) {
        r.push(name + " THROW " + e.name + " " + log.join());
    }
}
var log = [];
function iter(vals) {
    var i = 0;
    var it = {};
    it[Symbol.iterator] = function() { log.push("iter"); return it };
    it.next = function() {
        log.push("step" + i);
        return i < vals.length ? {value: vals[i++], done: false} : {done: true};
    };
    it["return"] = function() { log.push("return"); return {} };
    return it;
}
function base(o, tag) { log.push(tag); return o }
function key(tag, s) { log.push(tag); return {toString: function() { log.push("conv" + tag); return s }} }
function src(o) {
    var p = {};
    Object.keys(o).forEach(function(k) {
        Object.defineProperty(p, k, {enumerable: true, get: function() { log.push("get" + k); return o[k] }});
    });
    return p;
}
function def(v) { log.push("def"); return v }
t("array index", function() { log = []; var o = {}; [base(o, "o")[key("k", "a")]] = iter([1]); return log.join() + "|" + o.a })
t("array member", function() { log = []; var o = {}; [base(o, "o").a, base(o, "p").b] = iter([1, 2]); return log.join() + "|" + o.a + o.b })
t("array default", function() { log = []; var o = {}; [base(o, "o")[key("k", "a")] = def(5)] = iter([undefined]); return log.join() + "|" + o.a })
t("array hole", function() { log = []; var o = {}; [, base(o, "o")[key("k", "a")]] = iter([1, 2]); return log.join() + "|" + o.a })
t("array rest", function() { log = []; var o = {}; [base(o, "o").x, ...base(o, "p")[key("k", "r")]] = iter([1, 2, 3]); return log.join() + "|" + o.x + JSON.stringify(o.r) })
t("array nested", function() { log = []; var o = {}; [[base(o, "o")[key("k", "a")]], {b: base(o, "p").b}] = iter([iter([1]), src({b: 2})]); return log.join() + "|" + o.a + o.b })
t("object", function() { log = []; var o = {}; ({a: base(o, "o")[key("k", "x")], b: base(o, "p").y} = src({a: 1, b: 2})); return log.join() + "|" + o.x + o.y })
t("object computed", function() { log = []; var o = {}; ({[key("s", "a")]: base(o, "o")[key("k", "x")]} = src({a: 1})); return log.join() + "|" + o.x })
t("object default", function() { log = []; var o = {}; ({a: base(o, "o")[key("k", "x")] = def(7)} = src({a: undefined})); return log.join() + "|" + o.x })
t("object rest", function() { log = []; var o = {}; var s = src({b: 2}); s.a = 1; ({a: base(o, "o").a, ...base(o, "p")[key("k", "r")]} = s); return log.join() + "|" + o.a + JSON.stringify(o.r) })
t("object nested", function() { log = []; var o = {}; ({a: [base(o, "o").x], b: {c: base(o, "p").y}} = src({a: iter([1]), b: src({c: 2})})); return log.join() + "|" + o.x + o.y })
t("target throws closes", function() { log = []; [base({}, "o")[(function() { log.push("k"); throw new TypeError() })()]] = iter([1]); return log.join() })
t("null base", function() { log = []; [base(null, "o")[key("k", "a")]] = iter([1]); return log.join() })
function* gen() {
    var o = {};
    [base(o, "o")[key("k", "a")] = yield 1, base(o, "p").b] = iter([undefined, 2]);
    return log.join() + "|" + o.a + o.b;
}
t("generator", function() { log = []; var g = gen(); g.next(); return g.next(9).value })
var result = JSON.stringify(r);
