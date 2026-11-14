// Well-formed JSON.stringify (ECMA-262 §25.5.2.3 QuoteJSONString): an unpaired surrogate is
// written as a `\uXXXX` escape (lower-case hex), a pair as itself. V4 6.12 copies the lone
// unit raw, hence no `// v4:` pin; checked against the spec by hand. Array.from of a string
// iterates code points (§23.1.2.1 uses String.prototype[@@iterator], §22.1.5.1): a lone
// surrogate is one element and a pair one; V4 6.12 splits the pair.
var result = [
    JSON.stringify("\udead") === '"\\udead"',
    JSON.stringify(["\udc00x\ud800"]) === '["\\udc00x\\ud800"]',
    JSON.stringify({ k: "a\ud83d" + "\ude00b\udbff" }) === '{"k":"a\ud83d\ude00b\\udbff"}',
    JSON.stringify("\ud800").length,
    JSON.parse(JSON.stringify("\ud800")).charCodeAt(0),
    Array.from("a\udc00\ud83d\ude00").map(function (c) { return escape(c) }).join()
].join("|");
