// Call argument evaluation order: callee base object (and computed key) before arguments.
// v4: [["f","2","[object Object]","3","4","[object Object]","g","5","6","[object Object]","[object Object]","7","8","3,1,2","1","3,1,2","3"],[1,2],7,11,15,true,true,[1,2,3],6]
var log = [];
function t(x) { log.push(x); return x; }
var o = {
    f: function () { log.push("f"); return [1]; },
    g: function (a, b) { return a + b; },
    k: "g"
};
var r1 = o.f().concat(t(2));
var r2 = t(o).g(t(3), t(4));
var r3 = t(o)[t("g")](t(5), t(6));
var r4 = t(o)[t(o).k](t(7), t(8));
var n = null;
var r5 = n?.g(t("skipped"));
var r6 = n?.[t("skippedKey")](t("skippedArg"));
var arr = [3, 1, 2];
var r7 = t(arr).slice(t(1)).concat(t(arr).length);
var s = { v: 1, inc: function () { this.v++; return this; } };
var r8 = s.inc().inc().v + t(s.v);
var result = JSON.stringify([log.map(String), r1, r2, r3, r4, r5 === undefined, r6 === undefined, r7, r8]);
