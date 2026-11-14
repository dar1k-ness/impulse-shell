// A named function expression's own name is an immutable binding of its scope: a sloppy write
// to it is ignored and a strict one throws TypeError (ECMA-262 §15.2.5, §9.1.1.1.5). When a
// parameter list has expressions, every formal starts uninitialized, so a default reading a
// later formal throws ReferenceError (§10.2.11 step 28). QV4 lets the writes through and reads
// the later formal as undefined, which is not pinned.
var r = [];
r.push((function n() { n = 1; return typeof n; })());
r.push((function n() { n += 1; n++; return typeof n; })());
r.push((function n() { 'use strict'; try { n = 1; } catch (e) { return e.name; } return 'no'; })());
r.push((function n() {
    return (() => { 'use strict'; try { n = 1; } catch (e) { return e.name; } return 'no'; })();
})());
r.push((function n() { return () => { n = 5; return typeof n; }; })()());
r.push((function n() { var n = 2; n = 3; return n; })());
r.push((function n(n) { n = 4; return n; })(0));
r.push((function* n() { n = 1; yield typeof n; })().next().value);
r.push((function* n() { 'use strict'; try { n = 1; } catch (e) { yield e.name; } })().next().value);
var keep = function n() { return n; };
r.push(keep() === keep);
function f(a = b, b) { return 'ran'; }
try { f(); r.push('no'); } catch (e) { r.push(e.name); }
r.push(f(1));
function h(a, b = a, [c] = [b]) { return a + b + c; }
r.push(h(1));
function k({x} = {x: y}, y) { return x; }
try { k(); r.push('no'); } catch (e) { r.push(e.name); }
r.push(k({x: 7}));
function late(a = () => b, b = 2) { return a(); }
r.push(late());
function rest(a = r2, ...r2) { return a; }
try { rest(); r.push('no'); } catch (e) { r.push(e.name); }
var result = JSON.stringify(r.map(String));
