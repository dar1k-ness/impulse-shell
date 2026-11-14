var g = 0
let gl = 0
function setG() { g = 5; return g }
function setGl() { gl = 6; return gl }
function undeclared() { jsfoo = 1; return typeof jsfoo }
function undeclaredStrict() { "use strict"; jsfoo2 = 1; return typeof jsfoo2 }
function compound() { jsfoo3 += 1; return typeof jsfoo3 }
function local() { var a; a = 1; let b; b = 2; return a + b }
function closure() { var c = 0; (function() { c = 3 })(); return c }
function forIn() { for (jk in {a: 1}) {} return typeof jk }
function destructure() { [jd] = [1]; return typeof jd }
function thisWrite() { this.jt = 1; return typeof this.jt }
function builtin() { Math = 1; return typeof Math }
function undef() { undefined = 1; return typeof undefined }
function fnDecl() { setG = 1; return typeof setG }
