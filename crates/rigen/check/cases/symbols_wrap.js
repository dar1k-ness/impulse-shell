// v4: ["object",true,false,"Symbol(a)",true,"[object Symbol]","object","symbol","[{}]",true,"object",0,true,"object"]
var s = Symbol("a");
var w = Object(s);
Symbol.prototype.sloppy = function () { return typeof this };
Symbol.prototype.strict = function () { "use strict"; return typeof this };
var result = JSON.stringify([typeof w, w == s, w === s, w.toString(), w.valueOf() === s, Object.prototype.toString.call(w), s.sloppy(), s.strict(), JSON.stringify([w]), w instanceof Symbol, typeof new Object(s), Object.keys(w).length, w.constructor === Symbol, typeof Object.prototype.valueOf.call(s)]);
