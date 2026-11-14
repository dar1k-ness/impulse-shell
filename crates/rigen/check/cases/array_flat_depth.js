// `flat(depth)` flattens `depth` levels (default 1, Infinity for all) and skips holes.
// Qt 6.12's V4 has no Array.prototype.flat: the pin is ECMA-262's (and Node's) result.
// expect: [[1,2,[3,[4]],5],[1,[2,[3,[4]]],5],[1,2,3,[4],5],[1,2,3,4,5],[1,[2,[3,[4]]],5],[1,2,[3,[4]],5],0,[1,2,3]]
var a = [1, [2, [3, [4]]], , 5];
var r = [a.flat(), a.flat(0), a.flat(2), a.flat(Infinity), a.flat(-1), a.flat("1"), [[[]]].flat(Infinity).length,
    [[1, , 2], [3]].flat()];
var result = JSON.stringify(r);
