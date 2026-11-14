// A binding's value is the completion value of its statement, as QV4 computes it: the last
// expression statement run, through `switch` (with and without `break`), `try`/`catch`/
// `finally`, loops, labels and nested blocks; a loop, a `switch` and a `break`/`continue`
// that run no expression of their own give undefined.
// qt: "other" "one" 5 1 "yes" 6 2 7 "t" "in-block" "t" undefined 1 undefined undefined undefined undefined "blk" "one" "inner" "c" undefined undefined 2 undefined 7 6 undefined "end" 12 4 2 3 "r" 2 30
import QtQuick
Item {
    id: root
    property int a: 1
    property var arr: [1, 2, 3]
    property var c: switch (a) { case 0: "zero"; case 1: "one"; case 2: "two"; default: "other"; }
    property var d: switch (a) { case 0: "zero"; break; case 1: "one"; break; default: "other"; }
    property var e: try { nope.x } catch (e) { 5 }
    property var f: { a; }
    property var g: { if (a) "yes"; else "no" }
    property var h: { let q = 5; q + 1 }
    property var i: { for (let k = 0; k < 3; k++) k }
    property var j: { a: 7 }
    property var k: if (a) "t"
    property var l: { switch (a) { case 1: "in-block" } }
    property var m: { try { "t" } finally { "f" } }
    property var n: { var q = 5 }
    property var o: { 1; var q = 5 }
    property var p: { while (false) {} }
    property var q: { 3; if (false) 4 }
    property var s1: switch (a) { case 1: break; default: "d" }
    property var s2: switch (a) { case 5: "five" }
    property var s3: switch (a) { case 1: { "blk"; break } default: "d" }
    property var s4: switch (a) { default: "d"; case 1: "one" }
    property var s5: switch (a) { case 1: switch (a) { case 1: "inner"; break } break; default: "d" }
    property var t1: try { nope.x } catch (e) { "c" } finally { "f" }
    property var t2: try { 1; nope.x; 2 } catch (e) { }
    property var t3: { 1; try { throw 0 } catch (e) { } finally { 9 } }
    property var l1: { 3; for (var i = 0; i < 3; i++) { if (i == 1) continue; i } }
    property var l2: { 3; for (var i = 0; i < 3; i++) { if (i == 2) break; i } }
    property var l3: { 3; do { 7 } while (false) }
    property var l4: { 3; for (var x of arr) x * 2 }
    property var l5: { 3; while (false) 4 }
    property var l6: { var i = 0; do { i++; if (i < 2) continue; "end" } while (i < 3) }
    property var l7: { 1; for (var i = 0; i < 3; i++) { try { i; if (i == 1) continue; 10 + i } finally { 99 } } }
    property var b1: { 3; lbl: { 4; break lbl; 5 } }
    property var b2: { 3; lbl: for (var i = 0; i < 3; i++) { i; continue lbl; } }
    property var b3: { 1; lbl: { 2; for (;;) { 3; break lbl; } } }
    property var r1: { if (a) return "r"; "after" }
    property var r2: { 1; try { return 2 } finally { 3 } }
    Rectangle { id: r; width: switch (root.a) { case 0: 10; case 1: 20; default: 30; } }
    function show(v) { return v === undefined ? "undefined" : JSON.stringify(v) }
    property string result: [c, d, e, f, g, h, i, j, k, l, m, n, o, p, q, s1, s2, s3, s4, s5, t1, t2, t3, l1, l2, l3, l4, l5, l6, l7, b1, b2, b3, r1, r2, r.width].map(show).join(" ")
}
