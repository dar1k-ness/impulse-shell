// Audit case, classes: constructor order and `new.target`, `super` calls and members, static
// members, accessors, fields. The pin is what Qt 6.12's V4 gives, but for `o.q` (V4 makes
// `__proto__: {...}` of an object literal an own property, so `super.p` is undefined there;
// here it is `1`, ECMA-262's).
// expect: ["pre","AB",6,"B>A.m5","B>A.s","Bg>g5",8,"Bg>g8","pre","AC",6,true,"C",true,"ReferenceError","AD","ReferenceError: super() already called.","{\"z\":1}","ReferenceError","AH","TypeError","TypeError","function",true,true,1,"function","A",1,"","Named","Inner","undefined","42",43,"L!",6,3,true,false,true,true,"boom","N: boom","call",1,true,"constructor,m,g",0,true,1,null,"cg","1,2,3"]
var log = [];
class A { constructor(x) { this.x = x; log.push("A" + new.target.name); } m() { return "A.m" + this.x; } static s() { return "A.s"; } get g() { return "g" + this.x; } set g(v) { this.x = v; } }
class B extends A { constructor() { log.push("pre"); super(5); this.y = this.x + 1; log.push(this.y); } m() { return "B>" + super.m(); } static s() { return "B>" + super.s(); } get g() { return "Bg>" + super.g; } set g(v) { super.g = v * 2; } }
var b = new B(); log.push(b.m(), B.s(), b.g); b.g = 4; log.push(b.x, b.g);
class C extends B { }
var c = new C(); log.push(c instanceof A, c.constructor.name, Object.getPrototypeOf(C) === B);
class D extends A { constructor() { try { this.x; } catch (e) { log.push(e.name); } super(1); } }
new D();
class E extends A { constructor() { var f = () => super(9); f(); log.push(this.x); } }
try { new E(); } catch (e) { log.push(e.name + ": " + e.message); }
class F extends A { constructor() { return { z: 1 }; } }
log.push(JSON.stringify(new F()));
class G extends A { constructor() { } }
try { new G(); } catch (e) { log.push(e.name); }
class H extends A { constructor() { super(1); return 5; } }
try { new H(); } catch (e) { log.push(e.name); }
try { A(); } catch (e) { log.push(e.name); }
class I { ["comp" + 1]() { return 1; } static [Symbol.hasInstance](v) { return v === 7; } }
log.push(typeof I.prototype.comp1, 7 instanceof I);
var o = { m() { return super.toString === Object.prototype.toString; }, __proto__: { p: 1 }, get q() { return super.p; } };
log.push(o.m(), o.q);
log.push(typeof A, A.name, A.length, (class { }).name, (class Named { }).name);
var K = class Inner { who() { return Inner.name; } }; log.push(new K().who(), typeof Inner);
class L { toString() { return "L!"; } valueOf() { return 42; } }
log.push(`${new L()}`, new L() + 1, String(new L()));
class M extends Array { sum() { return this.reduce((a, b) => a + b, 0); } }
var m = new M(); m.push(1, 2, 3); log.push(m.sum(), m.length, Array.isArray(m), m.map(x => x * 2) instanceof M);
class N extends Error { constructor(msg) { super(msg); this.name = "N"; } }
try { throw new N("boom"); } catch (e) { log.push(e instanceof N, e instanceof Error, e.message, String(e)); }
function Old() { if (!new.target) return "call"; this.v = 1; }
log.push(Old(), new Old().v);
class P { static create() { return new this(); } } class Q extends P { }
log.push(Q.create() instanceof Q);
log.push(Object.getOwnPropertyNames(A.prototype).join(), Object.keys(A.prototype).length);
class R { constructor() { this.f = function () { return new.target === undefined; }; } }
log.push(new R().f());
class S extends null { static t() { return 1; } } log.push(S.t(), Object.getPrototypeOf(S.prototype));
var acc = { get [`k${1}`]() { return "cg"; } }; log.push(acc.k1);
class T { *gen() { yield 1; yield* [2, 3]; } }
log.push([...new T().gen()].join());
var result = JSON.stringify(log);
