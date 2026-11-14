// Spec over V4 (which cannot parse class fields): a symbol-keyed field is a DefineField of that
// symbol (ECMA-262 §7.3.33) and NamedEvaluation names an anonymous function `[description]`,
// or "" for a symbol without one (§10.2.9 SetFunctionName).
var s = Symbol("k"), u = Symbol();
class C {
    [s] = function () {};
    [u] = () => 1;
    static [s] = 2;
}
var c = new C();
var result = JSON.stringify([typeof c[s], c[s].name, c[u].name, C[s], Object.getOwnPropertySymbols(c).length, Object.keys(c).length, Object.getOwnPropertySymbols(C).length]);
