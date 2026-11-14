// ToPropertyKey (ECMA-262 §7.1.19) on every computed key: member reads, writes and calls,
// delete, `in`, object literal, class and pattern keys, assignment and for-in targets. An object
// key converts through ToPrimitive (toString, then valueOf; Object(symbol) to its symbol) where
// GetValue/PutValue need it: `o[k] = v` converts after `v` (§13.15.2), a call before its
// arguments. Where V4 orders the conversion differently: property_keys_spec.js.
// v4: ["get => 3","set => 3,3,true","valueOf => 3","toPrimitive => pstring","symbol wrapper => 1,0","number key => 6","array write => 3,9","literal => 1","method => function,7","static => 8","getter => 9","assign order => v,k,1","call => 1,k,arg,call","call builtin => 3-1-2","call symbol wrapper => 4","delete => true,false,k","in => true","pattern => 5,k","destructure target => x,k,1","for-in target => q,2","null set => TypeError,v","throwing key => RangeError,0"]
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
t("get", function() { var o = {zz: 3}; return o[key([], "zz")] })
t("set", function() { var o = {}; var k = key([], "zz"); o[k] = 3; return o.zz + "," + o[k] + "," + (k in o) })
t("valueOf", function() { var o = {}; o[{toString: null, valueOf: function() { return "vv" }}] = 3; return o.vv })
t("toPrimitive", function() { var o = {}; var k = {}; k[Symbol.toPrimitive] = function(h) { return "p" + h }; o[k] = 1; return Object.keys(o).join() })
t("symbol wrapper", function() { var o = {}; o[Object(sym)] = 1; return o[sym] + "," + Object.keys(o).length })
t("number key", function() { var a = [5, 6]; return a[{toString: function() { return "1" }}] })
t("array write", function() { var a = []; a[{toString: function() { return "2" }}] = 9; return a.length + "," + a[2] })
t("literal", function() { var o = {}; o = {[{toString: function() { return "q" }}]: 1}; return o.q })
t("method", function() { class C { [{toString: function() { return "m" }}]() { return 7 } } return typeof C.prototype.m + "," + new C().m() })
t("static", function() { class C { static [{toString: function() { return "s" }}]() { return 8 } } return C.s() })
t("getter", function() { var o = {get [{toString: function() { return "g" }}]() { return 9 }}; return o.g })
t("assign order", function() { var log = []; var o = {}; o[key(log, "a")] = (log.push("v"), 1); return log.join() + "," + o.a })
t("call", function() { var log = []; var o = {a: function(x) { log.push("call"); return this === o && x }}; return o[key(log, "a")]((log.push("arg"), 1)) + "," + log.join() })
t("call builtin", function() { return [3, 1, 2][{toString: function() { return "join" }}]("-") })
t("call symbol wrapper", function() { var o = {}; o[sym] = function() { return 4 }; return o[Object(sym)]() })
t("delete", function() { var log = []; var o = {a: 1}; var d = delete o[key(log, "a")]; return d + "," + ("a" in o) + "," + log.join() })
t("in", function() { var o = {a: 1}; return key([], "a") in o })
t("pattern", function() { var log = []; var o = {a: 5}; var {[key(log, "a")]: x} = o; return x + "," + log.join() })
t("destructure target", function() { var log = []; var o = {}; [o[key(log, "a")]] = [(log.push("x"), 1)]; return log.join() + "," + o.a })
t("for-in target", function() { var o = {}; var n = 0; for (o[{toString: function() { n++; return "a" }}] in {p: 1, q: 2}) {} return o.a + "," + n })
t("null set", function() { var log = []; try { null[key(log, "a")] = (log.push("v"), 1) } catch (e) { return e.name + "," + log.join() } })
t("throwing key", function() { var o = {}; try { o[{toString: function() { throw new RangeError("x") }}] = 1 } catch (e) { return e.name + "," + Object.keys(o).length } })
var result = JSON.stringify(r);
