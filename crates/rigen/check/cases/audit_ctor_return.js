// Audit case: what `new` returns when the constructor returns an object, a primitive or
// nothing, in functions and classes.
// v4: ["function",true,1,true,"function",true,true,true,true,"fin",true]
var log = [];
function A() { this.x = 1; return function f() {}; }
function B() { this.x = 1; return [1, 2]; }
function C() { this.x = 1; return 5; }
class D { constructor() { this.x = 1; return [7]; } }
class E { constructor() { this.x = 1; return function () {}; } }
class F extends D { constructor() { super(); log.push(Array.isArray(this)); } }
log.push(typeof new A(), Array.isArray(new B()), new C().x, Array.isArray(new D()), typeof new E());
new F();
class G { constructor() { return new Date(0); } }
log.push(new G() instanceof Date);
class H { constructor() { return /x/; } }
log.push(new H() instanceof RegExp);
class I { constructor() { return new Map(); } }
log.push(new I() instanceof Map);
class J { constructor() { try { return [1]; } finally { log.push("fin"); } } }
log.push(Array.isArray(new J()));
var result = JSON.stringify(log);
