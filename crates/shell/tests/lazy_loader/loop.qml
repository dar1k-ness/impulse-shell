import QtQuick
Item {
    id: root
    property var n: ({ a: 0, b: 0, c: 0 })
    function counts() { return "a " + n.a + " b " + n.b + " c " + n.c }
    property int k: 0
    property int a: { n.a++; return b + k + 1 }
    property int b: { n.b++; return a + 1 }
    property int c: { n.c++; return a * 2 }
    onAChanged: console.log("a", a)
    onBChanged: console.log("b", b)
    onCChanged: console.log("c", c)
    Component.onCompleted: console.log("init", a, b, c, counts())
    property int si: 0
    Timer { interval: 60; repeat: true; running: true; onTriggered: {
        if (root.si == 3) { console.log("done"); running = false; Qt.quit(); return }
        root.k = [1, 5, 5][root.si]; console.log("k", root.k, root.a, root.b, root.c, root.counts()); root.si++
    } }
}
