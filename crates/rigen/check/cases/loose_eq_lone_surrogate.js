// `==` converts an object to a primitive whatever the other operand's string holds (ECMA-262
// §7.2.14): a string with a lone surrogate too.
// v4: [[true,true,false,false],[true,true,true,true]]
var out = [];
function t(fn) {
    try { out.push(fn()); } catch (e) { out.push("THROWS " + e.name); }
}
t(function () { var o = { toString() { return "\ud800"; } }; return [o == "\ud800", "\ud800" == o, o != "\ud800", o == "\ud801"]; });
t(function () { var o = { valueOf() { return "a\udfff"; } }; return [o == "a\udfff", "a\udfff" == o, ["\ud800"] == "\ud800", new String("\ud800") == "\ud800"]; });
var result = JSON.stringify(out);
