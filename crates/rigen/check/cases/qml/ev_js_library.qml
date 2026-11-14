// The namespace object of a `.js` import is the file's live top-level names: its `var`s and
// functions are its properties, both sides see the other's writes, `let`, `const` and classes
// can only be read, the qualifiers of the file's `.import`s are not shown, and a `.import`
// of a QML module gives the file that module's types.
// qt: [1,2,["object","object","object","object","object","object","undefined","number","undefined","object","function",0,true,"undefined"],"undefined","undefined","undefined"] [["arr","bump","decl","fa","getX","lex","qq","redefine","self","setLate","und","x"],["arr","bump","decl","fa","getX","lex","qq","redefine","self","setLate","und","x"],12] ["number","number","function",[3,4,"function"],false,true,true,false,true,false,null,true,true] [3,3,3,4,4] [10,11,11,11] [101,101] [1,2,11,false,101] THROWS Error: Invalid write to global property "late" THROWS Error: Invalid write to global property "fresh" THROWS Error: Invalid write to global property "lx" THROWS Error: Invalid write to global property "cx" ["new","new"] ["replaced"] [false,"object"] ["object","[object Object]",false,"object",true,"number","undefined",true,true,true,"[object Object]"] [11,true,true,false,"undefined"] [5,true,6] [5] 8 ["inc","y"]
import QtQuick
import "ev_lib_a.js" as A
import "ev_lib_b.js" as B2
import "ev_lib_a.js" as A2
Item {
    id: root
    property string result: ""
    function t(fn) {
        var r
        try { r = JSON.stringify(fn()) } catch (e) { r = "THROWS " + e.name + ": " + e.message }
        result += (result ? " " : "") + r
    }
    function run() {
        t(() => [A.x, A.fa(), A.qq(), typeof A.B, typeof A.QQ, typeof A.QM])
        t(() => [Object.keys(A), Object.getOwnPropertyNames(A), Object.values(A).length])
        t(() => [typeof A.lx, typeof A.cx, typeof A.K, A.lex(), "lx" in A, "x" in A, "fa" in A, "B" in A, A.hasOwnProperty("x"), A.hasOwnProperty("lx"), A.und, "und" in A, Object.getOwnPropertyDescriptor(A, "lx") === undefined])
        t(() => [B2.inc(), A.fa(), B2.y, B2.inc(), B2.y])
        t(() => { A.x = 10; return [A.x, A.bump(), A.x, A.getX()] })
        t(() => { B2.y = 100; return [B2.inc(), A.fa()] })
        t(() => [A2.x, A2.bump(), A.x, A === A2, A2.fa()])
        t(() => [typeof A.late, A.setLate(), typeof A.late])
        t(() => { A.fresh = 1; return 0 })
        t(() => { A.lx = 9; return 0 })
        t(() => { A.cx = 9; return 0 })
        t(() => [A.redefine(), A.fa()])
        t(() => { A.getX = function() { return "replaced" }; return [A.getX()] })
        t(() => { var d = delete A.arr; return [d, typeof A.arr] })
        t(() => [typeof A, String(A), JSON.stringify(A) === JSON.stringify(A2), typeof A.self(), A.self() === A, A.decl(), typeof A.inner, Object.getPrototypeOf(A) === Object.prototype, A instanceof Object, Object.isExtensible(A), Object.prototype.toString.call(A)])
        t(() => { var d = Object.getOwnPropertyDescriptor(A, "x"); return [d.value, d.writable, d.enumerable, d.configurable, typeof d.get] })
        t(() => { Object.defineProperty(A, "x", {value: 5}); return [A.x, A.getX !== undefined, A.bump()] })
        t(() => { Object.defineProperty(A, "nn", {value: 5}); return [A.nn] })
        t(() => { "use strict"; A.x = 7; return A.bump() })
        t(() => { var ks = []; for (var k in B2) ks.push(k); return ks })
    }
    Component.onCompleted: run()
}
