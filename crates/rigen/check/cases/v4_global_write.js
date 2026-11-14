// An assignment to a name declared nowhere throws (QML forbids creating globals).
// v4: ["Error: Invalid write to global property \"undeclaredName\"","Error: Invalid write to global property \"loopVar\"","ok",2,"ok","undefined","undefined"]
var declared = 1;
function msg(f) { try { f(); return "ok" } catch (e) { return e.name + ": " + e.message } }
var r = [];
r.push(msg(function () { undeclaredName = 1 }));
r.push(msg(function () { for (loopVar = 0; loopVar < 2; loopVar++) {} }));
r.push(msg(function () { declared = 2 }), declared);
r.push(msg(function () { var local; local = 3 }));
r.push(typeof undeclaredName, typeof loopVar);
var result = JSON.stringify(r);
