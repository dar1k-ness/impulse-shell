// new.target in class field initializers (ECMA-262 §15.7.10: each one is a method of its
// own, so GetNewTarget is `undefined` there, even in an arrow), while the constructor
// around them still sees its NewTarget. Qt 6.12's V4 has no class fields, so no v4 pin.
var r = [];
function t(name, f) {
    try {
        r.push(name + " => " + f());
    } catch (e) {
        r.push(name + " THROW " + e.name);
    }
}
class A { constructor() { this.nt = new.target } }
t("field", function() { class B { x = new.target } return new B().x === undefined })
t("field arrow", function() { class B { x = (() => new.target)() } return new B().x === undefined })
t("derived field", function() { class B extends A { x = new.target; y = this.nt } var o = new B(); return (o.x === undefined) + "," + (o.y === B) })
t("derived field explicit", function() { class B extends A { x = new.target; constructor() { super(); this.c = new.target } } var o = new B(); return (o.x === undefined) + "," + (o.c === B) })
t("computed key", function() { var k; function H() { class B { [k = new.target === H ? "h" : "u"] = 1 } this.o = new B() } new H(); return k })
var result = JSON.stringify(r);
