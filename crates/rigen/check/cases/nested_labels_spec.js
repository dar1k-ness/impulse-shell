// ECMA-262 §14.13.4 LabelledEvaluation: every label of a nested chain joins the label set,
// so `continue a` of `a: b: for` continues that loop (LoopContinues §14.7.1.2) and
// `break a` of `a: b: { }` ends that block. No `// v4:` pin: Qt 6.12 V4 rejects `continue a`
// here ("continue outside of loop"), against the spec.
function forLoop() { var n = 0; a: b: for (var i = 0; i < 3; i++) { n++; continue a; } return n; }
function doWhile() { var n = 0; a: b: c: do { n++; if (n < 3) continue a; } while (n < 5); return n; }
function forOf() { var s = ""; a: b: for (var x of [1, 2, 3]) { s += x; continue a; } return s; }
function forIn() { var s = ""; a: b: for (var k in { p: 1, q: 2 }) { s += k; continue b; } return s; }
function whileLoop() {
    var i = 0, s = "";
    x: y: while (i < 4) { i++; if (i % 2) continue x; s += i; }
    return s;
}
function fromInner() {
    var out = [];
    outer: mid: for (var i = 0; i < 3; i++) {
        for (var j = 0; j < 3; j++) {
            if (j == 1) continue outer;
            if (i == 2) break mid;
            out.push(i + "" + j);
        }
    }
    return out.join(",");
}
function letLoop() {
    var fs = [];
    a: b: for (let i = 0; i < 3; i++) { fs.push(function () { return i; }); continue a; }
    return fs.map(function (f) { return f(); }).join(",");
}
function throughFinally() {
    var log = [];
    a: b: for (var i = 0; i < 2; i++) { try { continue a; } finally { log.push("f" + i); } }
    a2: b2: for (var j = 0; j < 5; j++) { try { if (j == 1) break a2; } finally { log.push("g" + j); } }
    return log.join(",");
}
function blocks() {
    var s = "a";
    p: q: { s += "b"; break p; s += "x"; }
    r: t: { s += "c"; break t; s += "y"; }
    return s;
}
var result = JSON.stringify([forLoop(), doWhile(), forOf(), forIn(), whileLoop(), fromInner(), letLoop(), throughFinally(), blocks()]);
