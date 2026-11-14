// `finally` runs when binding the catch parameter throws (ECMA-262 §14.15.3: the Catch
// clause's completion, abrupt or not, goes through the Finally block).
// v4: [["fin","TypeError"],["fin","TypeError"],["fin","ret"],["fin","RangeError"],["fin0","fin1"]]
var out = [];
function t(fn) {
    try { out.push(fn()); } catch (e) { out.push("THROWS " + e.name); }
}
t(function () { var r = []; try { try { throw 1; } catch ({ a: { b } }) { r.push("c"); } finally { r.push("fin"); } } catch (e) { r.push(e.name); } return r; });
t(function () { var r = []; try { try { throw 1; } catch ([a]) { r.push("c"); } finally { r.push("fin"); } } catch (e) { r.push(e.name); } return r; });
t(function () { var r = []; function g() { try { throw null; } catch ({ a }) { r.push("c"); } finally { r.push("fin"); return "ret"; } } r.push(g()); return r; });
t(function () { var r = []; try { try { throw { a: 1 }; } catch ({ a, b = (function () { throw new RangeError("x"); })() }) { r.push("c"); } finally { r.push("fin"); } } catch (e) { r.push(e.name); } return r; });
t(function () { var r = []; for (var i = 0; i < 2; i++) { try { throw undefined; } catch ({ x }) { r.push("c"); } finally { r.push("fin" + i); if (i == 0) continue; break; } } return r; });
var result = JSON.stringify(out);
