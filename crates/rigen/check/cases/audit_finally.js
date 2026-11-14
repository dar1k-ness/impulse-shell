// Audit case: `finally` overriding `return`, `break`, `continue` and `throw`, in loops,
// labels and generators.
// v4: ["gfin","{\"value\":9,\"done\":true}","f","b0","f0","f1","f2","00","01","10","11","20","f5a","ret","after","f9","in","out","k1","k2","sw","t","f",2,"d","in",2,14,2,"x",3,1,2,"s","nob","0,1"]
var log = [];
function f1() { try { return "t"; } finally { log.push("f"); } }
function f2() { try { throw 1; } catch (e) { return "c"; } finally { return "f"; } }
function f3() { for (var i = 0; i < 3; i++) { try { if (i == 1) continue; if (i == 2) break; log.push("b" + i); } finally { log.push("f" + i); } } return i; }
function f4() { outer: for (var i = 0; i < 3; i++) { for (var j = 0; j < 3; j++) { try { if (j == 1) continue outer; if (i == 2) break outer; } finally { log.push(i + "" + j); } } } return "d"; }
function f5() { try { try { throw new Error("in"); } finally { log.push("f5a"); } } catch (e) { return e.message; } }
function f6() { var i = 0; while (true) { try { return i; } finally { i++; if (i < 3) continue; } } }
function f7() { try { return log.push("ret"); } finally { log.push("after"); } }
function f8() { l: try { return 1; } finally { break l; } return 2; }
function f9() { do { try { throw 1; } catch (e) { break; } finally { log.push("f9"); } } while (true); return "x"; }
function f10() { try { throw 1; } catch (e) { try { throw 2; } catch (e2) { return e + e2; } finally { log.push("in"); } } finally { log.push("out"); } }
function f11() { var x = 1; try { return x; } finally { x = 2; } }
function f12() { for (var k of [1,2,3]) { try { if (k == 2) return k; } finally { log.push("k" + k); } } }
function* g1() { try { yield 1; yield 2; } finally { log.push("gfin"); } }
var it = g1(); it.next(); log.push(JSON.stringify(it.return(9)));
function f13() { switch (1) { case 1: try { break; } finally { log.push("sw"); } case 2: log.push("no"); } return "s"; }
function f14() { try { throw 1 } catch (e) { return "nob" } }
function f15() { let r = []; for (let i = 0; i < 2; i++) { try { continue } finally { r.push(() => i) } } return r.map(f => f()).join() }
log.push(f1(), f2(), f3(), f4(), f5(), f6(), f7(), f8(), f9(), f10(), f11(), f12(), f13(), f14(), f15());
var result = JSON.stringify(log);
