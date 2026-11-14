// Tagged templates (ECMA-262 §13.3.11, GetTemplateObject §13.2.8.4).
// v4: ["3|3|a|a|b|b|c|c|1|2","3|3|||||||1|2","1|1|\nAB\\ `|\\n\\x41\\u0042\\\\ \\`",true,true,true,[true,true,2,"0,1","[\"p\",\"q\"]","0,1",true,true,false,"object"],"7:m1","7:n2","7ctrue","2|2|call|call|||3","2|2|outer|outer|end|end|2|2|inner|inner|||4",["1","2","tag","tag","tag","3","tag","4","tag","tag","tag"],3,["a","b","get","call"],"1|1||"]
var log = [];
function t(x) { log.push(x); return x; }
function tag(strs) {
    log.push("tag");
    var out = [strs.length, strs.raw.length];
    for (var i = 0; i < strs.length; i++) out.push(strs[i] === undefined ? "U" : strs[i], strs.raw[i]);
    for (var j = 1; j < arguments.length; j++) out.push(arguments[j]);
    return out.join("|");
}
function id(strs) { return strs; }
var r1 = tag`a${t(1)}b${t(2)}c`;
var r2 = tag`${1}${2}`;
var r3 = tag`\n\x41\u0042\\ \``;
function site() { return id`same`; }
var sameSite = site() === site();
var otherSite = id`same` !== id`same`;
var loop = [];
for (var k = 0; k < 3; k++) loop.push(id`x${k}`);
var loopSame = loop[0] === loop[1] && loop[1] === loop[2];
var s = id`p${0}q`;
var shape = [Array.isArray(s), Array.isArray(s.raw), s.length, Object.keys(s).join(","),
    JSON.stringify(s), Object.keys(s.raw).join(","), "raw" in s, s.hasOwnProperty("raw"),
    Object.prototype.propertyIsEnumerable.call(s, "raw"), typeof s.raw];
var o = {
    v: 7,
    f: function (strs, x) { return this.v + ":" + strs[0] + x; },
    mk: function () { var self = this; return function (strs) { return self.v + strs[0] + (this === undefined || this === globalThisOrUndefined()); }; }
};
function globalThisOrUndefined() { return (function () { return this; })(); }
var r5 = o.f`m${1}`;
var r6 = o["f"]`n${2}`;
var r7 = o.mk()`c`;
var r8 = (function () { return tag; })()`call${t(3)}`;
var nested = tag`outer${tag`inner${t(4)}`}end`;
var order = [];
var p = { get g() { order.push("get"); return function (s, a, b) { order.push("call"); return a + b; }; } };
var r9 = p.g`${(order.push("a"), 1)}${(order.push("b"), 2)}`;
var empty = tag``;
var result = JSON.stringify([r1, r2, r3, sameSite, otherSite, loopSame, shape, r5, r6, r7, r8, nested,
    log.map(String), r9, order, empty]);
