// ToNumber of an array is ToNumber(ToPrimitive(join)) (ECMA-262 §7.1.4): a one-element array
// converts through its element's string, so booleans give NaN.
// v4: ["NaN","NaN","0","NaN","NaN","7","2","Infinity","0","0","0","NaN","NaN","NaN","true"]
var result = JSON.stringify([
  Math.abs([true]), Math.abs([false]), Math.abs([" "]), +[true], Math.floor([[true]]),
  Math.abs(["-7"]), Math.abs([[-2]]), 1 / Math.min([-0], 0), Math.abs([null]), Math.abs([undefined]),
  Math.abs([]), Math.abs([1, 2]), Math.max([true], 1), Math.pow([false], 1), isNaN([true])
].map(String));
