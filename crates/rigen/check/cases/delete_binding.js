// `delete` of a declared binding is false and leaves it bound; of an undeclared name, true.
// V4 gives true for a `var` or function at the top level of a library, which it also leaves
// bound.
// v4: [[false,false,false,false,false,2,4,3,"function"],true,[true,true,1,"function"]]
function g(p) {
    var y = 2;
    let z = 3;
    function h() {}
    return [delete y, delete p, delete z, delete h, delete arguments, y, p, z, typeof h];
}
function u() { return delete notDeclaredAnywhere }
var top = 1;
function topFn() {}
var deletedTop = [delete top, delete topFn, top, typeof topFn];
var result = JSON.stringify([g(4), u(), deletedTop]);
