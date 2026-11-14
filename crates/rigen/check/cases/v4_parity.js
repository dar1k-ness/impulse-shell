// Runtime semantics pinned to Qt's V4 engine (expected output from qmltestrunner, Qt 6).
// v4: [["1",0,2,"2",3,5,"3",6,8],[true,2,false,0],[true,2,true,4,false,0],[1,5],[["a","a"],0,"bXb",0],2,[10,["_v","v"],"{\"_v\":5,\"v\":10}",7,"dyn",["dyn1","n"],["dyn",1],10,["_v","v"],"dyn",true,true],[true,true,"TypeError",true],["TypeError: bad",true,true,"TypeError","bad",[],"{}","x","function",true,"RangeError: r","Error",true,true,false],[true,false,true,true,false,false],"{}",[]]
var re = /a(\d)/g, s = "a1 a2 a3", m, out = [];
while ((m = re.exec(s)) !== null) out.push(m[1], m.index, re.lastIndex);
var sticky = /x/y; sticky.lastIndex = 1;
var st = [sticky.test("ax"), sticky.lastIndex, sticky.test("ax"), sticky.lastIndex];
var g2 = /b/g; var t2 = [g2.test("abcb"), g2.lastIndex, g2.test("abcb"), g2.lastIndex, g2.test("abcb"), g2.lastIndex];
var ne = /a/; ne.lastIndex = 5; var t3 = [ne.exec("xa").index, ne.lastIndex];
var g3 = /a/g; g3.lastIndex = 3; var t4 = ["a1a2".match(g3), g3.lastIndex, "aXa".replace(g3, "b"), g3.lastIndex];
var u = /é(.)/g; u.exec("ééx"); var t5 = u.lastIndex;
var o = { _v: 1, get v() { return this._v * 2 }, set v(x) { this._v = x + 1 } }; o.v = 4;
var only = { get g() { return 7 } }; only.g = 9;
var k = "dyn", c = { get [k + "1"]() { return k }, n: 1 };
var sp = Object.assign({}, o), spread = Object.assign({}, c);
var acc = [o.v, Object.keys(o), JSON.stringify(o), only.g, c.dyn1, Object.keys(c), Object.values(c), sp.v, Object.keys(sp), spread.dyn1, o.hasOwnProperty("v"), "v" in o];
var te = new TypeError("bad"), caught;
try { null.x } catch (e) { caught = [e instanceof TypeError, e instanceof Error, e.name, e.constructor === TypeError] }
var errs = [String(te), te instanceof Error, te instanceof TypeError, te.name, te.message, Object.keys(te), JSON.stringify(te), TypeError("x").message, typeof TypeError, new Error("e") instanceof Error, String(new RangeError("r")), String(new Error()), new SyntaxError("s") instanceof SyntaxError, te.hasOwnProperty("message"), te.hasOwnProperty("name")];
var is = [Object.is(NaN, NaN), Object.is(0, -0), Object.is(-0, -0), Object.is("a", "a"), Object.is({}, {}), Object.is(null, undefined)];
var result = JSON.stringify([out, st, t2, t3, t4, t5, acc, caught, errs, is, JSON.stringify(/a/g), Object.keys(/a/g)]);
