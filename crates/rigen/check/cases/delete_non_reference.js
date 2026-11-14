// `delete` of an expression that is no reference still evaluates it.
// v4: [true,true,2]
var n = 0;
function f() { n++; return 1 }
var r = [delete f(), delete (n++, 5), n];
var result = JSON.stringify(r);
