pragma Singleton
pragma ComponentBehavior: Bound
import QtQuick 2.15
import "lib.js" as Lib

QtObject {
    id: s
    property bool on: true
    property int n: -1
    property double d: 1.5e3
    property url u: "a.png"
    property color c: "#fff"
    property var v
    property var e: []
    property var o: { "k": 1 }
    property var blk: { let q = 5; q + 1 }
    property var cond: if (on) 1; else 2
    property var sw: switch (n) { case 0: "z"; default: "o" }
    property var t: try { n.x } catch (e) { 5 }
    property var neg: - 1
    property var kw: s.get + s.set + s.of + s.from
    final property int fin: 1
    readonly property var rx: /[a-z]+/.test("a")
    property var cls: (class A extends Object { constructor() { super() } static m() { return 1 } get p() { return 2 } })
    property var chain: s?.on ? s.n : (s.d, s.u)
    property var num: 0x1F + 0b11 + 0o17 + 1_000 + .5 + 5.
    property var str: "a\n\u00e9\x41" + 'q\'' + "\
"
    property var tagged: String.raw`a${1}b`
    property var inc: n++ + ++n - n-- - --n
    property var bits: ~n & 1 | 2 ^ 3
    property var cmp: n < 1 && n <= 2 || n > 3 && n >= 4 || n == 5 || n != 6 || n === 7 || n !== 8
    property var asg: (n += 1, n -= 1, n *= 2, n /= 2, n %= 3, n **= 2, n <<= 1, n >>= 1, n >>>= 1, n &= 1, n |= 1, n ^= 1, n)
    property var inst: s instanceof QtObject && "on" in s
    property var nw: new Date(0).getTime()
    property var arr: [, 1, , ...e, ]
    property var sp: Math.max(...e)
    property var gen: (function* () { yield })
    property var lbl: { a: 7 }
    property var obj2: ({ get x() { return 1 }, set x(v) {}, [n]: 1, f() {}, *g() {}, n })
    function withs() { with (s) { return on } }
    function iter() { for (var k in o) {} for (const x of e) {} for (;;) break; while (0) ; if (0) ; else ; debugger }
    onNChanged: n = 2
    onOnChanged: function () { n = 1 }
    onDChanged: () => { n = 3 }
    onUChanged: v => v
}
