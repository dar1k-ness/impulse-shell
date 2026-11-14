// RegExp symbol methods where Qt 6.12's V4 deviates or lacks them: @@match/@@replace/@@search
// run a subclass's `exec` (RegExpExec, ECMA-262 §22.2.7.1), String.prototype.matchAll and
// RegExp.prototype[@@matchAll] make a RegExp String Iterator over a species copy (§22.1.3.14,
// §22.2.6.9, §22.2.9), replaceAll and matchAll reject a non-global RegExp, the replace function
// gets the groups object, `RegExp(re)` is `re` and a regexp-like pattern gives its source and
// flags (§22.2.4.1), and startsWith/endsWith/includes reject a RegExp (IsRegExp, §7.2.8).
// Also where V4 deviates: String's replace/search dispatch to @@replace/@@search, @@match reads
// `flags` and runs a regexp-like's `exec` per match, `search()` is the empty pattern's 0, and
// a /u empty match advances past a surrogate pair.
function thrown(f) { try { f(); return "none" } catch (e) { return e.name } }
var custom = {};
custom[Symbol.replace] = function (s, r) { return "R" + s + r };
custom[Symbol.search] = function (s) { return s.length * 10 };
var dispatch = ["x".replace(custom, "y"), "abcd".search(custom), "abc".search(),
    "\ud83d\ude00".replace(/(?:)/gu, "-")].join();
var log = [];
var fake = {
    get flags() { log.push("flags"); return "g" },
    lastIndex: 0,
    exec: function (s) {
        log.push("exec" + this.lastIndex);
        if (this.lastIndex >= 2) return null;
        var r = ["x"]; r.index = this.lastIndex; this.lastIndex++;
        return r;
    }
};
var fakes = [String(RegExp.prototype[Symbol.match].call(fake, "abc")), log.join()];
log = []; fake.lastIndex = 0;
fakes.push(RegExp.prototype[Symbol.replace].call(fake, "abc", "Y"), log.join());
log = []; fake.lastIndex = 3;
fakes.push(RegExp.prototype[Symbol.search].call(fake, "abc"), fake.lastIndex, log.join());
class R extends RegExp { exec(s) { R.n = (R.n || 0) + 1; var r = super.exec(s); if (r) r[0] = r[0].toUpperCase(); return r } }
var subclass = [String("aba".match(new R("a", "g"))), "aba".replace(new R("a", "g"), "[$&]"),
    "xa".search(new R("a")), R.n].join();
var it = "a1b22c".matchAll(/\d+/g);
var all = [];
for (var x of it) all.push(x[0] + "@" + x.index);
var y = /(?<d>\d)/g; y.lastIndex = 2;
var copy = y[Symbol.matchAll]("1234");
var first = copy.next().value;
class S extends RegExp {}
S.n = 0;
var sp = /a/g; sp.constructor = function (p, f) { S.n++; return new RegExp(p, f) };
sp.constructor[Symbol.species] = sp.constructor;
var spAll = Array.from("aXa".matchAll(sp)).length;
var empty = Array.from("\ud83d\ude00".matchAll(/(?:)/gu)).map(function (m) { return m.index });
var custom2 = {}; custom2[Symbol.matchAll] = function (s) { return "MA" + s };
var iter = [all.join(), Object.prototype.toString.call(it), typeof it.next, it[Symbol.iterator]() === it,
    first[0], first.groups.d, first.index, y.lastIndex, S.n, spAll, empty.join(), "s".matchAll(custom2),
    thrown(function () { "a".matchAll(/a/) }), JSON.stringify(Array.from("ab".matchAll("."), function (m) { return m[0] })),
    typeof Symbol.matchAll, String.prototype.matchAll.length, RegExp.prototype[Symbol.matchAll].name,
    JSON.stringify(it.next())].join("|");
var named;
"aXb".replace(/(?<n>X)/, function () { named = arguments[arguments.length - 1]; return "" });
var replaces = ["a.b.c".replaceAll(".", "$&!"), "abc".replaceAll("", "_"), "aXbX".replaceAll(/x/gi, "-"),
    thrown(function () { "a".replaceAll(/a/, "") }), named.n, Object.getPrototypeOf(named) === null,
    JSON.stringify(/a/.exec("a").groups)].join("|");
var m = /(?<y>\d{4})-(?<m>\d\d)/.exec("on 2024-05");
var re = /q+/g;
var like = { source: "z+", flags: "i" }; like[Symbol.match] = true;
var noMatch = /b/; noMatch[Symbol.match] = false;
var ctor = [RegExp(re) === re, new RegExp(re) === re, RegExp(re, "i") === re, String(RegExp(like)),
    String(new RegExp(like, "g")), thrown(function () { "/a/".startsWith(/a/) }),
    thrown(function () { "a".includes(/a/) }), "/b/".endsWith(noMatch), thrown(function () { "x".match(noMatch) }),
    "aXb".replace(/(?<n>X)/, "[$<n>]"), "aXb".replace(/(X)/, "[$<n>]"), m.groups.y, m.groups.m].join("|");
var result = JSON.stringify([dispatch, fakes, subclass, iter, replaces, ctor]);
