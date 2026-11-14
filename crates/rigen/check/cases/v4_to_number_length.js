// Assigning an object to an array's length converts it through valueOf/toString first.
// v4: ["1:1","2","RangeError"]
var r = [];
var a = [1, 2, 3]; a.length = {valueOf: function () { return 1; }}; r.push(a.length + ':' + a);
a = [1, 2, 3]; a.length = {toString: function () { return '2'; }, valueOf: undefined}; r.push(a.length);
try { a.length = {valueOf: function () { return 1.5; }}; } catch (e) { r.push(e.name); }
var result = JSON.stringify(r.map(String));
