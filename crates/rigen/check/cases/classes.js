class A {
  x = 1;
  static count = 0;
  constructor(v) { this.v = v; A.count++ }
  get double() { return this.v * 2 }
  set double(d) { this.v = d / 2 }
  m() { return "A" + this.v }
  static make() { return new A(9) }
}
class B extends A {
  y = this.x + 1;
  constructor(v) { super(v + 1); this.z = 3 }
  m() { return "B" + super.m() }
}
class C extends B { }
const b = new B(1), c = new C(5);
const a = A.make(); a.double = 20;
function F(n) { this.n = n }
var result = JSON.stringify([b.m(), b.x, b.y, b.z, b.double, c.m(), c.y, A.count, a.v, b instanceof A, new F(4).n, typeof A]);
