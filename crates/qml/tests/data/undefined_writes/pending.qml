import QtQuick
Item {
    id: root
    function tt(f) { try { var v = f(); return typeof v + ":" + v } catch (e) { return "!" + e } }
    property int currentIndex: 0
    property string seen: ""
    property real seenWidth: -1
    Component { id: maker; Item {} }
    Loader {
        id: loader
        active: false
        sourceComponent: Row {
            id: row
            height: 32
            Repeater {
                model: 2
                delegate: Item {
                    id: seg
                    required property int index
                    readonly property bool current: index === root.currentIndex
                    implicitWidth: label.implicitWidth + 28
                    width: implicitWidth
                    height: row.height
                    onCurrentChanged: {
                        const w = seg.width, iw = seg.implicitWidth, h = seg.height;
                        root.seen += typeof w + " " + typeof iw + " " + typeof h + ";";
                        try { root.seenWidth = seg.width } catch (e) { root.seen += e + ";" }
                    }
                    Item { id: label; implicitWidth: 10; property var made: maker.createObject(label) }
                }
            }
        }
    }
    Component.onCompleted: loader.active = true
    property int tick: 0
    Timer { interval: 1; running: true; onTriggered: root.tick++ }
    property var cases: [
        () => root.seen,
        () => root.seenWidth
    ]
    property string result: (tick, cases.map(f => tt(f)).join(" | "))
}
