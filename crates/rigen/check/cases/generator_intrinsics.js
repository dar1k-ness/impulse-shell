// %GeneratorFunction%, %GeneratorFunction.prototype% and %GeneratorPrototype% (ECMA-262
// §27.3, §27.5): their links, @@toStringTag and property attributes, and the one-argument
// `next`, `return` and `throw`. (QV4 also leaves %GeneratorFunction%'s [[Prototype]]
// %Function.prototype%, not %Function% (§27.3.2); the interpreter unit tests cover that.)
// v4: ["GeneratorFunction","Generator","function","false","GeneratorFunction","1","true","true","true","1","1","1","next","return","throw","[object GeneratorFunction]","true","false/false/true","false/false/true","false/false/true","false/false/true","false/false/true","false/false/false","false/false/true"]
var GF = Object.getPrototypeOf(function* () {});
var GP = GF.prototype;
var G = GF.constructor;
var d = function (o, k) {
    var x = Object.getOwnPropertyDescriptor(o, k);
    return [x.writable, x.enumerable, x.configurable].join('/');
};
var r = [
    GF[Symbol.toStringTag], GP[Symbol.toStringTag], typeof G, G === Function, G.name, G.length,
    G.prototype === GF, GP.constructor === GF,
    Object.getPrototypeOf(GF) === Function.prototype, GP.next.length, GP.return.length,
    GP.throw.length, GP.next.name, GP.return.name, GP.throw.name,
    Object.prototype.toString.call(function* () {}), (function* () {}) instanceof G,
    d(GF, Symbol.toStringTag), d(GP, Symbol.toStringTag), d(GF, 'constructor'),
    d(GF, 'prototype'), d(GP, 'constructor'), d(G, 'prototype'), d(G, 'length'),
];
var result = JSON.stringify(r.map(String));
