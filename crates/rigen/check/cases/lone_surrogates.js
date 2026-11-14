// A String is any sequence of UTF-16 code units (ECMA-262 §6.1.4), so an unpaired surrogate
// survives literals, String.fromCharCode/fromCodePoint, slicing a pair, concatenation (which
// rejoins the halves), comparison, iteration, the string methods, JSON.parse escapes
// (§25.5.1) and Map keys; Encode (§19.2.6.5) throws URIError on one. Results are escape()d
// so no lone surrogate reaches the case's own JSON. JSON.stringify of one (well-formed
// stringify, §25.5.2.3) and Array.from of a string (code points, §23.1.2.1 via @@iterator)
// are in lone_surrogates_spec_over_v4.js: V4 6.12 writes the unit raw and splits the pair.
// v4: ["1","55296","%uDC00","%uD800","%uD83D","%uDE00","%uDE00","%uD83D%7C%uDE00","true","128512","55296","false","true","true%2Ctrue%2Ctrue","x%uDC00y%uD800","%uD800b","a%uD8001","true","%uDC00%uDC00%uD800%uD800x","%uD800%uD800","2","a%7Cb","1%2C1%2Ctrue","true","%uD800A%uDC00b","%uD800%7C","%uD800","3","55296","throw URIError","throw URIError","%25F0%2590%2580%2580","%uD800","true","%uDC00x%2C%E9%uDBFF","true","undefined%2C1","%uDC00","x-y","a%uDC00%uD800","true","stringtrueNaN","1%3A%25uD800","%uDC00","0"]
function t(f) {
    try {
        var v = f();
        return typeof v === "string" ? escape(v) : String(v);
    } catch (e) {
        return "throw " + e.name;
    }
}
var result = JSON.stringify([
    t(function () { return "\ud800".length }),
    t(function () { return "\ud800".charCodeAt(0) }),
    t(function () { return String.fromCharCode(0xdc00) }),
    t(function () { return String.fromCodePoint(0xd800) }),
    t(function () { return "\ud83d\ude00".charAt(0) }),
    t(function () { return "\ud83d\ude00".slice(1) }),
    t(function () { return "\ud83d\ude00"[1] }),
    t(function () { return "\ud83d\ude00".substring(0, 1) + "|" + "\ud83d\ude00".substr(1) }),
    t(function () { return ("\ud83d" + "\ude00") === "\ud83d\ude00" }),
    t(function () { return ("\ud83d" + "\ude00").codePointAt(0) }),
    t(function () { return "\ud800".codePointAt(0) }),
    t(function () { return "\ud800" == "\ufffd" }),
    t(function () { return "\ud800" === String.fromCharCode(0xd800) }),
    t(function () { return ["\udead" < "\uffff", "\udead" > "\ud7ff", "\ud800" < "\ud800\udc00"].join() }),
    t(function () { return `x${"\udc00"}y` + `\ud800` }),
    t(function () { return ["\ud800", "b"].join("") }),
    t(function () { return String(["a\ud800"]) + 1 }),
    t(function () { return "\ud800".concat("\udc00") === "\ud800\udc00" }),
    t(function () { return "\ud800".padStart(3, "\udc00") + "\ud800".padEnd(2, "x") }),
    t(function () { return "\ud800".repeat(2) }),
    t(function () { return "a\ud800".split("").length }),
    t(function () { return "a\ud800b".split("\ud800").join("|") }),
    t(function () { return "x\ud800".indexOf("\ud800") + "," + "x\ud800".lastIndexOf("\ud800") + "," + "\ud800x".includes("x") }),
    t(function () { return "\ud800".startsWith("\ud800") && "a\udc00".endsWith("\udc00") }),
    t(function () { return "\ud800a".toUpperCase() + "\udc00B".toLowerCase() }),
    t(function () { return " \ud800 ".trim() + "|" }),
    t(function () { return "\ud800".normalize() }),
    t(function () { var n = 0; for (var c of "a\ud800\ud83d\ude00") n++; return n }),
    t(function () { return unescape("%uD800").charCodeAt(0) }),
    t(function () { return encodeURI("\udc00") }),
    t(function () { return encodeURIComponent("a\ud800b") }),
    t(function () { return encodeURI("\ud800\udc00") }),
    t(function () { return JSON.parse('"\\ud800"') }),
    t(function () { return JSON.parse('"\\ud800\\udc00"') === "\ud800\udc00" }),
    t(function () { return JSON.parse('{"a":["\\udc00x", "\\u00e9\\udbff"]}').a.join() }),
    t(function () { return JSON.stringify("\ud83d\ude00") === '"\ud83d\ude00"' }),
    t(function () { var m = new Map([["\ud800", 1]]); return m.get("\ufffd") + "," + m.get("\ud800") }),
    t(function () { return "x\udc00".match(/\udc00/)[0] }),
    t(function () { return "x\udc00y".replace(/\udc00/, "-") }),
    t(function () { return "ab\ud800".replace("b", "\udc00") }),
    t(function () { return /\ud800/.test("x\ud800") }),
    t(function () { return typeof "\ud800" + !!"\ud800" + (+"\ud800") }),
    t(function () { return Object("\ud800").length + ":" + escape(Object("\ud800")[0]) }),
    t(function () { return new String("\udc00").valueOf() }),
    t(function () { return "\ud800".localeCompare("\ud800") })
]);
