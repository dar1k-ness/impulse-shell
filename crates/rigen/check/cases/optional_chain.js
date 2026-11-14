// ECMA-262 OptionalChain: a short-circuit skips the rest of the chain, arguments and computed
// keys included; every call on the chain's spine is optional in V4.
// v4: [["f","nopeKey","1"],["f1","undefined","undefined","undefined","undefined","undefined","undefined","bc","undefined","undefined","undefined"],true,true]
var log = [];
function g(x) { log.push(x); return x; }
var a = { b: {}, s: "abc", f: function (x) { return x + 1; } };
var z = null;
var r = [
    a?.f(g("f")),
    a?.nope(g("nope")),
    a?.b.c(g("b.c")),
    z?.f(g("z.f")),
    z?.[g("z[]")],
    z?.[g("z[]call")](g("z[]arg")),
    a?.[g("nopeKey")](g("nopeKeyArg")),
    a?.s.slice(g(1)),
    a?.s.nope(g("s.nope")),
    a.b?.c?.d(g("b?.c?.d")),
    z?.f(g("deep")).x(g("deeper")),
];
var u = undefined;
var r2 = u?.(g("u()"));
var r3 = a?.b.c?.(g("c?.()"));
var result = JSON.stringify([log.map(String), r.map(String), r2 === undefined, r3 === undefined]);
