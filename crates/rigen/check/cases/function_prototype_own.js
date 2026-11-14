// A constructor has its own `prototype` before anything reads it; arrows, methods and bound
// functions have none.
// v4: [true,true,true,false,true,false,false,false,0,true]
function F() {}
function G() {}
var r = ["prototype" in F, G.hasOwnProperty("prototype"), Object.prototype.hasOwnProperty.call(function () {}, "prototype"),
    "prototype" in (() => 1), (function* () {}).hasOwnProperty("prototype"), ({ m() {} }).m.hasOwnProperty("prototype"),
    "prototype" in F.bind(null), "prototype" in Math.max, Object.keys(F).length, F.prototype.constructor === F];
var result = JSON.stringify(r);
