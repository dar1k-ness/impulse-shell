// A string literal's escapes are UTF-16 code units: an escaped surrogate pair is its code point,
// a lone surrogate U+FFFD under the UTF-8 string model (as for computed strings), never the
// escape's text. No `// v4:` pin: V4 keeps the lone unit.
function t(f) {
    try {
        var v = f();
        return typeof v === "string" ? JSON.stringify(v) : String(v);
    } catch (e) {
        return "throw " + e.name;
    }
}
var o = {"\ud83d": 1, "a\ud83d\ude00": 2};
var result = [
    t(function () { return "\ud83d".length; }),
    t(function () { return "\ud83d".charCodeAt(0); }),
    t(function () { return "\ud83d" === String.fromCharCode(0xd83d); }),
    t(function () { return ("\ud83d" + "\ude00").length; }),
    t(function () { return "\ud83d\ude00" === "\u{1f600}"; }),
    t(function () { return "\ud83d\ude00".length; }),
    t(function () { return "\uD83D\uDE00".codePointAt(0); }),
    t(function () { return "a\ud83db".length; }),
    t(function () { return "\ude00\ud83d".length; }),
    t(function () { return JSON.stringify("\udead"); }),
    t(function () { return `\ud83d`.length; }),
    t(function () { return String.raw`\ud83d`.length; }),
    t(function () { return Object.keys(o).join(",").length; }),
    t(function () { return o["\ufffd"]; }),
    t(function () { return o["a\u{1f600}"]; })
].join("|");
