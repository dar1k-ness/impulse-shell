// RegExp exec and test where Qt 6.12's V4 deviates: test runs a replaced `exec` (RegExpExec,
// ECMA-262 §22.2.7.1), exec reads and ToLengths `lastIndex` whatever the flags
// (RegExpBuiltinExec, §22.2.7.2), /i without the u flag canonicalizes by toUppercase but never
// from non-ASCII to ASCII (Canonicalize, §22.2.2.7.3), and without it `\u{41}` is `u` 41 times
// (IdentityEscape, Annex B §B.1.2).
var r = /a/;
r.exec = function () { return null };
var calls = [];
var generic = RegExp.prototype.test.call({ exec: function (s) { calls.push(s); return {} } }, "x");
var thrown = false;
try {
    RegExp.prototype.test.call({ exec: function () { return 1 } }, "x");
} catch (e) {
    thrown = e instanceof TypeError;
}
var n = 0, li = /a/;
li.lastIndex = { valueOf: function () { n++; return 0 } };
li.exec("a");
li.test("a");
var raised = "", bad = /a/;
bad.lastIndex = { valueOf: function () { throw new Error("lastIndex") } };
try {
    bad.exec("a");
} catch (e) {
    raised = e.message;
}
var icase = [/\u017f/i.test("s"), /s/i.test("\u017f"), /[a-z]/i.test("\u212a"), /\w/i.test("\u017f"),
    /\W/i.test("\u017f"), /[^k]/i.test("\u212a"), /(s)\1/i.test("s\u017f"), /\u017f/iu.test("s"),
    /[a-z]/iu.test("\u212a"), /\u00e9/i.test("\u00c9"), /\u03c3/i.test("\u03a3"),
    /\u03c2/i.test("\u03c3")];
var esc = [new RegExp("\\u{41}").test("A"), new RegExp("^\\u{2}$").test("uu"),
    new RegExp("[\\u{41}]").test("{"), new RegExp("\\u{41}", "u").test("A")];
var result = [r.test("a"), generic, calls.join(), thrown, n, raised, icase.join(), esc.join()].join("|");
