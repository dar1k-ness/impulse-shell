var vals = [0, -0, 1, -2.5, NaN, Infinity, "", "0", "1", "10", "9", "a", "B", "é", "\u{1F600}",
  null, undefined, true, false, [], [3], [1, 2], {}, { valueOf() { return 7 } }];
var out = [];
for (var i = 0; i < vals.length; i++) {
  var x = vals[i];
  out.push([-x, +x, !x, typeof x]);
  for (var j = 0; j < vals.length; j++) {
    var y = vals[j];
    out.push([x + y, x - y, x * y, x / y, x % y, x === y, x !== y, x == y, x != y,
      x < y, x > y, x <= y, x >= y]);
  }
}
var c = 5, s = "s";
c += 2; c -= 10; c *= -3; c /= 4; c %= 2; s += 1; s -= 1;
out.push(c, s);
var result = JSON.stringify(out);
