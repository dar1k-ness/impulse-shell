// `super.x = v` assigns through a Super Reference (ECMA-262 §13.3.7.3 MakeSuperPropertyReference,
// §6.2.5.6 PutValue): [[Set]] of the key on the home object's prototype with `this` as the
// receiver (§10.1.9.2 OrdinarySet), so a data property lands on `this` and a setter found there
// gets `this`. Class code is strict, so a refused [[Set]] (read-only in the chain, an accessor
// or read-only own property of `this`, a non-extensible or primitive `this`) throws a
// TypeError; so does a null super base.
// v4: [[true,5,0],[true,false],false,["A.s true 3"],"TypeError false","TypeError","TypeError 1","TypeError",[1,null],[8,9,2,false],"TypeError",["B","A",true],"ReferenceError","TypeError"]
function t(f) { try { return f() } catch (e) { return e.name } }
var log = [];
class A { set s(v) { log.push("A.s " + (this instanceof B) + " " + v) } }
A.prototype.x = 0;
A.prototype.q = 1;
A.prototype.w = 0;
Object.defineProperty(A.prototype, "ro", { value: 1, writable: false });
A.k = "A";
var sym = Symbol("s");
class B extends A {
  m() { super.x = 5; return [this.hasOwnProperty("x"), this.x, A.prototype.x] }
  idx(k) { super[k] = 7; return [this.hasOwnProperty(k), A.prototype.hasOwnProperty(k)] }
  callSetter() { super.s = 3; return this.hasOwnProperty("s") }
  ro() { return t(() => { super.ro = 2 }) + " " + this.hasOwnProperty("ro") }
  getterOnly() {
    Object.defineProperty(this, "q", { get() { return "g" }, configurable: true });
    return t(() => { super.q = 2 })
  }
  frozenOwn() {
    Object.defineProperty(this, "w", { value: 1, writable: false, configurable: true });
    return t(() => { super.w = 2 }) + " " + this.w
  }
  closed() { Object.preventExtensions(this); return t(() => { super.z = 1 }) }
  sym() { super[sym] = 1; return [this[sym], A.prototype[sym]] }
  destr() {
    [super.d1] = [8];
    ({ a: super.d2 } = { a: 9 });
    for (super.d3 of [1, 2]);
    return [this.d1, this.d2, this.d3, "d1" in A.prototype]
  }
  prim() { super.x = 1; return "set" }
  static sm() { super.k = "B"; return [B.k, A.k, B.hasOwnProperty("k")] }
}
class L extends A { constructor() { super.x = 1; super() } }
class N { m() { super.x = 1 } }
var b = new B();
var n = new N();
Object.setPrototypeOf(N.prototype, null);
var result = JSON.stringify([b.m(), b.idx("y"), b.callSetter(), log, b.ro(), b.getterOnly(),
  b.frozenOwn(), new B().closed(), b.sym(), b.destr(),
  t(() => B.prototype.prim.call(5)), B.sm(), t(() => new L()), t(() => n.m())]);
