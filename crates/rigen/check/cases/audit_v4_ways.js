// Two places of the audit's cases where this runtime follows Qt 6.12's V4 instead of ECMA-262:
// a function declared in a block is not visible after it (no Annex B.3.3 hoisting), and a
// strict write to an undeclared name throws V4's plain Error, not ReferenceError.
// v4: ["undefined","Error"]
function sh5() { { function blk() { return 1; } } return typeof blk; }
var strictWrite = (function () { "use strict"; try { undefinedVar = 1; } catch (e) { return e.name; } })();
var result = JSON.stringify([sh5(), strictWrite]);
