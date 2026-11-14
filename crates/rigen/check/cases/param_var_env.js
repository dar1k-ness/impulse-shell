// A function whose parameter list has expressions gets its own VarEnvironment for the body
// (ECMA-262 §10.2.11 steps 27-28): closures in the parameters see the formals and the outer
// scope, never the body's `var`/`let`/function declarations; a body `var` named like a formal
// (or `arguments`) is a separate binding that starts with the formal's value. QV4 6.12 shares
// one scope, so this is not pinned.
var r = [];
var x = 'outer';
function f1(a = () => x) { var x; return a(); }
r.push(f1());
function f2(a, b = () => a) { var a = 2; return [a, b()].join(); }
r.push(f2(1));
function f3(a, b = () => a) { var a; return a; }
r.push(f3(1));
function f4(x = 1, g = () => { x = 2; }, h = () => x) { var x; g(); return [x, h()].join(); }
r.push(f4());
var h = 'outer';
function f5(g = () => typeof h) { function h() {} return [typeof h, g()].join(); }
r.push(f5());
var y = 'outer';
function f6(g = () => y) { let y = 1; return [y, g()].join(); }
r.push(f6());
function f7(a, g = () => arguments) { var arguments; return arguments === g(); }
r.push(f7(1));
function f8(a, g = () => arguments) { var arguments = 5; return [arguments, typeof g(), g()[0]].join(); }
r.push(f8(1));
function f9(a = 1) { var a = 2; return (() => a)(); }
r.push(f9());
function f10(a, b = () => a) { a = 2; return b(); }
r.push(f10(1));
r.push((function () {
    'use strict';
    function f11(a, b = () => a) { var a = 3; return [a, b()].join(); }
    return f11(1);
})());
function* g12(a, b = () => a) { var a = 2; yield [a, b()].join(); }
r.push(g12(1).next().value);
function f13({p} = {}, q = () => p) { var p = 'body'; return [p, q()].join(); }
r.push(f13({p: 'arg'}));
function f14(a, b = () => a) { var a; { var a = 4; } return [a, b()].join(); }
r.push(f14(1));
function f15(a = 0, g = () => k) { var k = 1; try { return g(); } catch (e) { return e.name; } }
r.push(f15());
function f16(a = 0) { var a; return (function () { return a; })(); }
r.push(f16(6));
var result = JSON.stringify(r.map(String));
