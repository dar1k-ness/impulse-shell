import QtQuick
import QtQuick 2.15 as Q
import QtQuick.Layouts

Item {
    id: root
    property int count: 3
    property string title: "a" + 'b'
    readonly property real ratio: width / Math.max(1, height)
    default property list<Item> kids
    required property var model
    property alias w: root.width
    property var cb: function (a, b) { return a + b }
    property var arrow: (x, y = 2, ...rest) => x * y
    property var obj: ({ a: 1, "b": [1, 2, 3], c: { d: null } })
    property list<Item> items: [Item {}, Rectangle { color: "red" }]
    property Item child: Item { width: 1 }
    signal clicked(int x, string name)
    signal done
    signal typed(x: int, y: real)
    width: 100; height: parent ? parent.height : 0
    anchors.fill: parent
    anchors { left: parent.left; margins: 4 }
    onClicked: (x, name) => console.log(x, name)
    onDone: { count++; return }
    Component.onCompleted: {
        for (let i = 0; i < 3; i++) { if (i % 2) continue; else break }
        var s = `t ${count + 1} u ${`n ${title}`}`
        let re = /a[/]b+/gi, q = 4 / 2 / 1
        switch (count) { case 1: s = "x"; break; default: s = "y" }
        try { throw new Error("e") } catch (e) { s = e.message } finally { s += "!" }
        do { count-- } while (count > 0)
        label: while (true) { break label }
        const { a, b: [c, d = 1] = [] } = obj
        s = a ?? c?.d?.[0] ?? typeof d
        count = count > 1 ? count << 2 : count >>> 1
        delete obj.a, void 0
    }
    function f(a, b) { return a + b }
    function g(a: int, b: string): string { return b + a }
    function* gen() { yield 1; yield* [2, 3] }
    enum Mode { Off, On = 2, Auto = -1 }
    component Badge: Rectangle { radius: 2 }
    Behavior on width { NumberAnimation { duration: 100 } }
    Rectangle {
        id: r
        color: Qt.rgba(1, 0, 0, 1)
        states: [State { name: "a" }, State { name: "b" }]
        Q.Text { text: qsTr("hi") }
    }
    Badge {}
    Timer { interval: 100; running: true; onTriggered: root.done() }
}
