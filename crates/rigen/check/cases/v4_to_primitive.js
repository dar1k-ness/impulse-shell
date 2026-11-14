// ToPrimitive in numeric, relational, additive and equality operators (ECMA-262 §7.1.1).
// v4: [3,true,2,true,true,14,true,null,10,0,0,4,"71",11,"10","10",10,-3,7,-4,9,1,6,7,1,1.5,true,false,true,true,true,false,true,false,false,"TypeError","TypeError","TypeError","TypeError","a,b,a,b,a,b,a,b,a,b",[false,true,-1,1,3],2,3,null,null,0.5]
var o = { valueOf: function () { return 3 } };
var s = { toString: function () { return "7" } };
var both = { valueOf: function () { return 10 }, toString: function () { return "20" } };
var bad = { valueOf: function () { return {} }, toString: function () { return {} } };
function err(f) { try { return f() } catch (e) { return e.name } }
var order = [];
var a = { valueOf: function () { order.push("a"); return 1 } };
var b = { valueOf: function () { order.push("b"); return 2 } };
var cmp = [a > b, a < b, a - b, a ** b, a | b];
var x = { valueOf: function () { return 1 } };
x++;
var y = { valueOf: function () { return 5 } };
y -= 2;
var result = JSON.stringify([
  new Date(5) - new Date(2), new Date(9) > new Date(4), o - 1, o < 4, o > 2, s * 2, s < 8,
  [1, 2] - 1, [5] * 2, [undefined] - 0, [null] * 1, o + 1, s + 1, both + 1, both + "", "" + both,
  both * 1, -o, +s, ~o, o ** 2, o & 1, o << 1, s >>> 0, o % 2, o / 2, o >= 3, o <= 2,
  o == 3, s == "7", both == 10, new Date(5) == 5, [1] == 1, o === 3, 3 != o,
  err(function () { return bad - 1 }), err(function () { return bad < 1 }),
  err(function () { return bad + 1 }), err(function () { return -bad }),
  order.join(","), cmp, x, y, 1 ** NaN, (-1) ** Infinity, 2 ** -1
]);
