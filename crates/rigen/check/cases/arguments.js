// Mapped and unmapped arguments objects (ECMA-262 §10.4.4.6, §10.4.4.7).
// v4: [9,9,5,6,"1:3","4:undefined",1,"TypeError",1,1,9,"12:12","1:1",5,"false|true|0,1|undefined|{\"0\":1,\"1\":2}|2","1,2:1,2:0,1",1]
function setArg(a) { arguments[0] = 9; return a }
function setParam(a) { a = 9; return arguments[0] }
function closureWrite(a) { (function () { a = 5 })(); return arguments[0] }
function arrowWrite(a) { (() => arguments[0] = 6)(); return a }
function delThenArg(a) { delete arguments[0]; arguments[0] = 3; return a + ":" + arguments[0] }
function delThenParam(a) { delete arguments[0]; a = 4; return a + ":" + arguments[0] }
function strict(a) { "use strict"; arguments[0] = 9; return a }
function strictCallee() { "use strict"; try { return arguments.callee } catch (e) { return e.name } }
function withDefault(a = 0) { arguments[0] = 9; return a }
function withRest(a, ...r) { arguments[0] = 9; return a }
function dup(a, a) { arguments[1] = 9; return a }
function bump(a) { arguments[0]++; a += 10; return a + ":" + arguments[0] }
function redeclared(a) { var a; return a + ":" + arguments[0] }
function reassigned(a) { arguments = 5; a = 3; return arguments }
function shape(a) {
    return [Array.isArray(arguments), arguments.callee === shape, Object.keys(arguments).join(),
        typeof arguments.push, JSON.stringify(arguments), arguments.length].join("|")
}
function iter(a) {
    var r = [], ks = [];
    for (var x of arguments) r.push(x);
    for (var k in arguments) ks.push(k);
    return r.join() + ":" + [...arguments].join() + ":" + ks.join()
}
class C { m(a) { arguments[0] = 9; return a } }
var result = JSON.stringify([setArg(1), setParam(1), closureWrite(1), arrowWrite(1), delThenArg(1),
    delThenParam(1), strict(1), strictCallee(), withDefault(1), withRest(1), dup(1, 2), bump(1),
    redeclared(1), reassigned(1), shape(1, 2), iter(1, 2), new C().m(1)]);
