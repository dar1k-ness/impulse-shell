// `Math.pow` gives what `**` gives: NaN for a base of ±1 with an infinite or NaN exponent.
// v4: ["NaN","NaN","NaN","NaN","NaN","NaN","1","1024","1","NaN","NaN"]
var r = [Math.pow(1, Infinity), Math.pow(-1, Infinity), Math.pow(1, -Infinity), Math.pow(-1, -Infinity),
    Math.pow(1, NaN), Math.pow(-1, NaN), Math.pow(NaN, 0), Math.pow(2, 10), Math.pow(1, 2), Math.pow(-8, 1 / 3),
    1 ** Infinity];
var result = JSON.stringify(r.map(String));
