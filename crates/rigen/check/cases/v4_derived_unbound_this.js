// `this` of a derived constructor before `super(...)`, QV4's way: a store to its property
// runs the right-hand side and is a TypeError if `this` is still unbound, a call of its
// method runs the arguments and is no function of what QV4 holds there (its text depends
// on how Qt was built and is masked), `delete` of its property is true, a read is a
// ReferenceError with an empty name, and so is a call of its method by a computed key
// (before the key and the arguments); `super.x` there names the missing call. An arrow
// function there never has a `this` (`v4_derived_arrow.js`).
// v4: ["TypeError|Type error","TypeError|Type error",5,"ok:[object Object]","ReferenceError| is not defined","ReferenceError| is not defined","ReferenceError|Missing call to super().","ReferenceError|Missing call to super().","ReferenceError|Missing call to super().","ReferenceError|Missing call to super().","ReferenceError|this is not defined","TypeError|Property 'm' of object ? is not a function",6,"ReferenceError| is not defined","ReferenceError| is not defined","ReferenceError| is not defined","ok:true","ReferenceError|this is not defined","ReferenceError| is not defined","TypeError|Type error","ok:2,true,","ok:1","ok:true,true","ReferenceError|this is not defined","ReferenceError|this is not defined",6,"ReferenceError|this is not defined","TypeError|Type error","TypeError|Type error","TypeError|Property 'm' of object ? is not a function","TypeError|Type error","ReferenceError| is not defined","ReferenceError|this is not defined","ReferenceError|this is not defined","ReferenceError| is not defined",6,"ReferenceError|this is not defined"]
function kind(f) { try { return "ok:" + f() } catch (e) { return e.name + "|" + e.message.replace(/of object (0|null) is/, "of object ? is") } }
class A { constructor() { this.a = 1 } }
var r = [], side = 0;
r.push(kind(function () { class B extends A { constructor() { this.x = 1 } } return new B() }));
r.push(kind(function () { class B extends A { constructor() { this.x = (side = 5); super() } } return new B() }), side);
r.push(kind(function () { class B extends A { constructor() { this.x = super() } } return new B() }));
r.push(kind(function () { class B extends A { constructor() { this.x } } return new B() }));
r.push(kind(function () { class B extends A { constructor() { return this } } return new B() }));
r.push(kind(function () { class B extends A { constructor() { super.x } } return new B() }));
r.push(kind(function () { class B extends A { constructor() { super.x() } } return new B() }));
r.push(kind(function () { class B extends A { constructor() { super.x = 1 } } return new B() }));
r.push(kind(function () { class B extends A { constructor() { super["x"] } } return new B() }));
r.push(kind(function () { class B extends A { constructor() { var f = () => this; f() } } return new B() }));
r.push(kind(function () { class B extends A { constructor() { this.m((side = 6)) } m() {} } return new B() }), side);
r.push(kind(function () { class B extends A { constructor() { this[0] = 1 } } return new B() }));
r.push(kind(function () { class B extends A { constructor() { this.x++ } } return new B() }));
r.push(kind(function () { class B extends A { constructor() { this.x += 1 } } return new B() }));
r.push(kind(function () { class B extends A { constructor() { var d = delete this.x; super(); this.d = d } } return new B().d }));
r.push(kind(function () { class B extends A { constructor() { delete this.x } } return new B() }));
r.push(kind(function () { class B extends A { constructor() { this.x.y = 1 } } return new B() }));
r.push(kind(function () { class B extends A { constructor() { { this.x = 1 } } } return new B() }));
r.push(kind(function () { class B extends A { constructor() { super(); this.x = 2; this.m() } m() { this.y = delete this.a } } var b = new B(); return [b.x, b.y, b.a] }));
r.push(kind(function () { class B extends A { constructor() { this.m(super()) } m() { return 1 } } return new B().a }));
r.push(kind(function () { class B extends A { constructor() { var q = (this.x = super()); this.q = q === this } } var b = new B(); return [b.q, b.x === b] }));
r.push(kind(function () { class B extends A { constructor() { var f = () => { this.x = 1 }; f() } } return new B() }));
r.push(kind(function () { class B extends A { constructor() { var f = () => this.m((side = 9)); f() } m() {} } return new B() }), side);
r.push(kind(function () { class B extends A { constructor() { var f = () => delete this.x; f() } } return new B() }));
r.push(kind(function () { class B extends A { constructor() { [this.x] = [1] } } return new B() }));
r.push(kind(function () { class B extends A { constructor() { for (this.x of [1]) {} } } return new B() }));
r.push(kind(function () { class B extends A { constructor() { this.m`a` } m() {} } return new B() }));
r.push(kind(function () { class B extends A { constructor() { this.x = 1, super() } } return new B() }));
r.push(kind(function () { class B extends A { constructor() { this["m"]() } m() {} } return new B() }));
r.push(kind(function () { class B extends A { constructor() { var f = () => { this.x = 1 }; super(); f() } } return new B().x }));
r.push(kind(function () { class B extends A { constructor() { var f = () => this; super(); return {v: f()} } } return new B().v }));
r.push(kind(function () { class B extends A { constructor() { var k = "m"; this[k](side = 7) } m() {} } return new B() }), side);
r.push(kind(function () { class B extends A { constructor() { var f = () => () => this.a; super(); this.g = f() } } return new B().g() }));
var result = JSON.stringify(r);
