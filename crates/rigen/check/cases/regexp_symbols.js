// v4: ["Mx","1,2,-1,1,1,0","a[a|b|c|$]c|abb$2bb0c|k-j-al|a[$<n>]b|aXX14bXX34|a..b.c|_abc|acac|5|-a-a-a-","2024,05,3,1,22,,1,b,1,2,2"]
// String.prototype.match calls the argument's @@match (ECMA-262 §22.1.3.13), and RegExp's
// @@match/@@replace/@@search step `lastIndex` past empty matches, leave it alone in search
// and expand `$` patterns (§22.2.6.8, 11, 12; GetSubstitution, §22.1.3.19.1).
var custom = {};
custom[Symbol.match] = function (s) { return "M" + s };
var dispatch = "x".match(custom);
var re = /b/g; re.lastIndex = 2;
var y = /a/y; y.lastIndex = 1;
var searches = ["abcb".search(re), re.lastIndex, "ba".search(y), y.lastIndex, "xyz".search("y"),
    "a.b".search(".")].join();
var subst = ["abc".replace(/b/, "[$`|$&|$'|$$]"), "abc".replace(/(b)/, "$1$1$2$01$10"),
    "abcdefghijkl".replace(/(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)(k)/, "$11-$10-$1"),
    "aXb".replace(/(X)/, "[$<n>]"),
    "aXbX".replace(/(X)/g, function (m, p, i, s) { return m + p + i + s.length }),
    "a.b.c".replace(".", "$&$&"), "abc".replace("", "_"), "abc".replace("b", "$'$`"),
    "\ud83d\ude00".replace(/(?:)/g, "-").length,
    "aaa".replace(/a*?/g, "-")].join("|");
var m = /(?<y>\d{4})-(?<m>\d\d)/.exec("on 2024-05");
var matches = [m[1], m[2], m.index, String("a1b22".match(/\d+/g)), "ab".match(/x/g),
    "a1b2".match(/\d/).index, String("abc".match("b")), "".match(/(?:)/g).length,
    "\ud83d\ude00".match(/(?:)/gu).length, re.lastIndex].join();
var result = JSON.stringify([dispatch, searches, subst, matches]);
