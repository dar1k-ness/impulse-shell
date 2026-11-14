// A sloppy function without formals gets a mapped arguments object too (ECMA-262 §10.2.11
// step 22): its callee is the function, not the strict-mode thrower.
// v4: ["function",true,true,"0,1:2","TypeError"]
function callee() { return typeof arguments.callee }
function self() { return arguments.callee === self }
function inArrow() { return (() => arguments.callee === inArrow)() }
function keys() { var r = []; for (var k in arguments) r.push(k); return r.join(",") + ":" + arguments.length }
function strictNone() { "use strict"; try { return arguments.callee } catch (e) { return e.name } }
var result = JSON.stringify([callee(), self(), inArrow(), keys(4, 5), strictNone()]);
