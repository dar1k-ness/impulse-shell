// A binding goes on over a line break when the next line starts with a token that cannot
// start a member but can continue the expression (`!==`, `instanceof`, `in`, `^`, `(`, `[`,
// `,`), and an `if`'s `else`, a `try`'s `catch`/`finally` and a statement's body may start
// the next line.
// qt: true true true 6 20 4 7 4 4 4 5 7 three 4 20 3 5 3 4
import QtQuick
Item {
    id: root
    property int a: 3
    property int b: 4
    property var arr: [10, 20]
    function f(x) { return x * 2 }
    property bool c1: a
        !== b
    property bool c2: arr
        instanceof Array
    property bool c3: 1
        in arr
    property var c4: f
        (a)
    property var c5: arr
        [1]
    property var c6: a
        , b
    property var c7: a
        ^ b
    property var c8: if (a > b) a
        else b
    property var c9: if (a > b) a;
        else b
    property var c10: if (a > b) { a }
        else { b }
    property var c11: try { nope.x }
        catch (e) { 5 }
    property var c12: try { 7 }
        finally { 5 }
    property var c13: switch (a)
        { case 3: "three" }
    property var c14: if (a)
        b
    property var c15: arr // c
        /* d */ [1]
    property var c16: a
    property int inx: 5
    width: a
    height: b
    property string result: [c1, c2, c3, c4, c5, c6, c7, c8, c9, c10, c11, c12, c13, c14, c15, c16, inx, width, height].join(" ")
}
