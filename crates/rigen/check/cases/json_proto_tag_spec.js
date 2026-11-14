// JSON.stringify with a PropertyList and JSON.parse with a reviver (ECMA-262 §25.5.2,
// §25.5.1.1), `__proto__: v` in an object literal (Annex B §B.3.1) and Math/JSON
// @@toStringTag (§21.3.1.9, §25.5.3). Qt 6.12's V4 ignores the replacer array and the reviver
// and defines `__proto__` as an own property; the runtime follows the spec, so no `// v4:` pin.
// The expected values are pinned in qml's json_stringify_property_list, json_parse_reviver,
// literal_proto_sets_prototype and math_and_json_to_string_tag.
var r = [];
function t(name, f) {
    try {
        r.push(name + " => " + f());
    } catch (e) {
        r.push(name + " THROW " + e.name);
    }
}
t("list", function() { return JSON.stringify({b: 1, a: 2}, ["a"]) })
t("list kinds", function() { return JSON.stringify({1: "one", b: 2, a: 1, c: 3}, ["b", 1, new String("a"), "b", new Number(1), {}, true, null]) })
t("list nested", function() { return JSON.stringify({a: {a: 1, z: 2}, z: 0, y: [{a: 3, z: 4}]}, ["a", "y"]) })
t("list inherited", function() { return JSON.stringify(Object.create({a: 1}), ["a", "x"]) })
t("list proxy", function() { return JSON.stringify({a: 1, b: 2}, new Proxy(["b"], {})) })
t("list indent", function() { return JSON.stringify({a: 1, b: 2}, ["a"], 1) })
t("list not array", function() { return JSON.stringify({a: 1}, {0: "b", length: 1}) })
t("reviver", function() { return JSON.parse("[1,2]", function(k, v) { return typeof v === "number" ? v * 2 : v }).join() })
t("reviver order", function() { var log = []; JSON.parse('{"a":[1,{"b":2}],"c":3}', function(k, v) { log.push(k); return v }); return log.join() })
t("reviver delete", function() { var o = JSON.parse('{"a":1,"b":2}', function(k, v) { return k === "a" ? undefined : v }); return Object.keys(o).join() + ("a" in o) })
t("reviver hole", function() { var a = JSON.parse("[1,2,3]", function(k, v) { return k === "1" ? undefined : v }); return a.length + "," + (1 in a) })
t("reviver holder", function() { return JSON.parse('{"a":1}', function(k, v) { return k === "" ? (this[""] === v) + "," + Object.keys(this) : v }) })
t("reviver live", function() { return JSON.parse('{"a":1,"b":2}', function(k, v) { if (k === "a") this.b = 5; return v }).b })
t("reviver root", function() { return JSON.parse('{"a":1}', function(k, v) { return k === "" ? "root" : v }) })
t("reviver not callable", function() { return JSON.parse("[1]", 5)[0] })
t("reviver throws", function() { JSON.parse("[1]", function() { throw new RangeError("r") }) })
t("proto", function() { var p = {x: 1}; var o = {a: 1, __proto__: p, b: 2}; return [Object.getPrototypeOf(o) === p, o.hasOwnProperty("__proto__"), o.x, Object.getOwnPropertyNames(o).join(" ")].join() })
t("proto string", function() { var p = {}; return Object.getPrototypeOf({"__proto__": p}) === p })
t("proto null", function() { var o = {__proto__: null}; return [Object.getPrototypeOf(o), Object.getOwnPropertyNames(o).length].join() })
t("proto primitive", function() { var o = {__proto__: 5}; return [Object.getPrototypeOf(o) === Object.prototype, Object.getOwnPropertyNames(o).length].join() })
t("proto computed", function() { var p = {}; var o = {["__proto__"]: p}; return [Object.getPrototypeOf(o) === Object.prototype, o.__proto__ === p].join() })
t("proto shorthand", function() { var __proto__ = {}; var o = {__proto__}; return [Object.getPrototypeOf(o) === Object.prototype, o.hasOwnProperty("__proto__")].join() })
t("proto method", function() { var o = {__proto__() { return 1 }}; return [Object.getPrototypeOf(o) === Object.prototype, o.__proto__.name].join() })
t("proto getter", function() { var o = {get __proto__() { return 7 }}; return [Object.getPrototypeOf(o) === Object.prototype, o.__proto__].join() })
t("proto anonymous fn", function() { var o = {__proto__: function() {}}; return JSON.stringify(Object.getPrototypeOf(o).name) })
t("proto and computed", function() { var p = {}; var o = {__proto__: p, ["__proto__"]: 1}; return [Object.getPrototypeOf(o) === p, o.hasOwnProperty("__proto__")].join() })
t("proto order", function() { var log = []; var o = {a: log.push("a"), __proto__: (log.push("p"), {}), b: log.push("b")}; return log.join() })
t("proto super", function() { var o = {__proto__: {v: 7}, m() { return super.v }}; return o.m() })
t("proto pattern", function() { var a, b; ({__proto__: a, __proto__: b} = {}); return a === Object.prototype && b === a })
t("tags", function() { return [Object.prototype.toString.call(Math), String(JSON), Math[Symbol.toStringTag], JSON[Symbol.toStringTag]].join() })
t("tag desc", function() { var d = Object.getOwnPropertyDescriptor(Math, Symbol.toStringTag); var e = Object.getOwnPropertyDescriptor(JSON, Symbol.toStringTag); return [d.writable, d.enumerable, d.configurable, e.writable, e.enumerable, e.configurable].join() })
var result = JSON.stringify(r);
