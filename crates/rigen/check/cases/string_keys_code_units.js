// The own keys of a string are the indices of its UTF-16 code units (ECMA-262 §10.4.3.3): an
// astral character is two elements.
// v4: [[["0","1","2"],["0","1","2"],55357,56832],[["0","1","2"],3,3,["0","1","2","length"]],[["0","1"],1,55357,56832,56832,true,true,[97,55357,56832]],[["0","1"],2,["0","1"],[]]]
var out = [];
function t(fn) {
    try { out.push(fn()); } catch (e) { out.push("THROWS " + e.name); }
}
t(function () { var o = Object.assign({}, "a\ud83d\ude00"); return [Object.keys("a\ud83d\ude00"), Object.keys(o), o[1].charCodeAt(0), o[2].charCodeAt(0)]; });
t(function () { var ks = []; for (var k in "a\ud83d\ude00") ks.push(k); return [ks, Object.values("a\ud83d\ude00").length, Object.entries("a\ud83d\ude00").length, Object.getOwnPropertyNames("a\ud83d\ude00")]; });
t(function () { var s = "a\ud83d\ude00"; var o = Object.assign({}, "\ud83d\ude00"); return [Object.keys(o), o[0].length, o[0].charCodeAt(0), o[1].charCodeAt(0), Object.getOwnPropertyDescriptor(s, "2").value.charCodeAt(0), "2" in new String(s), new String(s).hasOwnProperty(2), Array.from(Object.values(s), c => c.charCodeAt(0))]; });
t(function () { return [Object.keys("\ud800a"), Object.entries("a\udc00").length, Object.keys(new String("\ud83d\ude00")), Object.keys("")]; });
var result = JSON.stringify(out);
