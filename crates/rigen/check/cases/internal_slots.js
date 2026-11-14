var o = { get a() { return this.c * 2 }, set a(v) { this.c = v }, c: 1 };
o.a = 5;
var keys1 = Object.keys(o).join();
var json1 = JSON.stringify(o);
delete o.a;
var w = { get x() { return 1 }, y: 2, x: 3 };
var d = new Date(5);
delete d["\x01time"];
d["\x01time"] = 7;
var m = new Map([[1, 2]]);
var s = { "\x01get z": function () { return 1 } };
class A { f() { return "A" } get g() { return "g" } }
class B extends A {
  f() { return super.f() + "B" }
  h() { return [1].map(() => super.f() + this.g)[0] }
}
var b = new B();
var result = JSON.stringify([keys1, json1, o.a, Object.keys(o).join(), Object.keys(w).join(), w.x, d.getTime(),
  Object.keys(d).length, Object.keys(m).length, m.get(1), s.z, Object.keys(s).length, b.f(), b.h(), b.g]);
