// Array methods convert their arguments before reading the array, against the length taken first:
// a valueOf that shrinks the array leaves holes for fill and splice to see.
// v4: ["1|2","1","1","1|9|9|9","2||","1"]
var a = [1, 2, 3, 4];
function cut(v) { return {valueOf: function () { a.length = 2; return v; }}; }
var r = [a.slice(cut(0), 4).join('|'), a.indexOf(2, cut(0))];
a = [1, 2, 3, 4]; r.push(a.lastIndexOf(2, cut(3)));
a = [1, 2, 3, 4]; r.push(a.fill(9, cut(1)).join('|'));
a = [1, 2, 3, 4]; r.push(a.splice(cut(1), 3).join('|'), a.join('|'));
var result = JSON.stringify(r.map(String));
