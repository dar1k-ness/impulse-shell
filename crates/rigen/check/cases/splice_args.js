// `splice()` deletes nothing; one argument deletes from `start` to the end.
// v4: [[],[1,2,3],[2,3],[1],[1,2,3],[],[],[1,2,3]]
var a = [1, 2, 3], b = [1, 2, 3], c = [1, 2, 3], d = [1, 2, 3];
var result = JSON.stringify([a.splice(), a, b.splice(1), b, c.splice(undefined), c, d.splice(-1, undefined), d]);
