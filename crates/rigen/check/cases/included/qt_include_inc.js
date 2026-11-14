var incVar = 1;
let incLet = 2;
class IncClass {}
function incFn(x) { return x * incVar + libLocal(); }
function incLexical() { return [incLet, typeof IncClass].join(); }
Qt.include("qt_include_nested.js");
