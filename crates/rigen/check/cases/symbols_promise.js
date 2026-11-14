// v4: ["[object Promise]","Promise",1]
// Promise.prototype[@@toStringTag] is "Promise" (ECMA-262 §27.2.5.5).
var p = Promise.resolve(1);
var result = JSON.stringify([Object.prototype.toString.call(p), String(Promise.prototype[Symbol.toStringTag]), Object.getOwnPropertySymbols(Promise.prototype).length]);
