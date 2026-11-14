.pragma library
var g = 0
let gl = 0
function setG() { g = 5; return g }
function setGl() { gl = 6; return gl }
function undeclared() { libfoo = 1; return typeof libfoo }
function undeclaredStrict() { "use strict"; libfoo2 = 1; return typeof libfoo2 }
function compound() { libfoo3 += 1; return typeof libfoo3 }
function local() { var a; a = 1; let b; b = 2; return a + b }
function closure() { var c = 0; (function() { c = 3 })(); return c }
function forIn() { for (lk in {a: 1}) {} return typeof lk }
function destructure() { [ld] = [1]; return typeof ld }
function thisWrite() { this.lt = 1; return typeof this.lt }
function builtin() { Math = 1; return typeof Math }
function undef() { undefined = 1; return typeof undefined }
function fnDecl() { setG = 1; return typeof setG }
function readBack() { return [typeof libfoo, typeof lk, typeof ld, typeof lt] }
