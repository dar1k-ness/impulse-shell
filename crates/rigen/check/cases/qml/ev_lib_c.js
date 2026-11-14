/* A directive is one only before the program's first token:
.import "Nope.js" as Z
*/
// .import "Nope.js" as Z
.pragma library
var c = 1
var s = "\
.import 'Nope.js' as Z"
function f() { return /* .pragma library */ c }
