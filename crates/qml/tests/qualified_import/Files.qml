import QtQuick as QQ
import "parts" as P
import "parts"

QQ.Item {
    id: root
    property P.Part typed: first
    property Part plain: first
    property list<P.Part> parts: [
        P.Part {
            n: 5
        }
    ]
    property int stored: P.Store.value
    property int hits: 0
    component Local: P.Part {
        n: 7
    }
    function cls(o) {
        return String(o).replace(/(_QMLTYPE_\d+)?(_QML_\d+)?\(.*$/, "")
    }
    function take(p: P.Part): int {
        return p.n
    }
    P.Part {
        id: first
        n: 1
        onPinged: k => console.log("pinged", k)
        QQ.Item {
            id: inside
        }
    }
    Part {
        id: second
    }
    Local {
        id: local
    }
    P.Rectangle {
        id: wrapped
    }
    QQ.Rectangle {
        id: real
    }
    Rectangle {
        id: bare
    }
    QQ.Connections {
        target: P.Store
        function onValueChanged() {
            root.hits++
        }
    }
    QQ.Timer {
        interval: 30
        running: true
        onTriggered: {
            console.log("ticks", first.ticks > 0, second.ticks, local.ticks > 0)
            Qt.quit()
        }
    }
    QQ.Component.onCompleted: {
        console.log("class", cls(first), cls(second), cls(local), cls(wrapped), cls(real), cls(bare))
        first.ping()
        console.log("typed", typed === first, plain === first, parts.length, parts[0].n, take(second), inside.parent !== first)
        console.log("inline", local.n)
        console.log("shadow", wrapped.wrapped, wrapped.radius, real.wrapped, real.radius, bare.wrapped, bare.radius)
        console.log("instanceof", first instanceof P.Part, first instanceof Part, second instanceof P.Part, local instanceof P.Part, first instanceof QQ.Item)
        console.log("instanceof shadow", wrapped instanceof QQ.Rectangle, wrapped instanceof P.Rectangle, typeof P.Part.Inner)
        console.log("as", (first as P.Part) === first, (first as QQ.Item) === first)
        console.log("enum", P.Part.Fast, P.Part.Mode.Fast, Part.Fast, typeof P.Part.Slow)
        P.Store.value = 9
        console.log("singleton", stored, hits, P.Store.value, P.Store === Store)
        var made = Qt.createQmlObject("import QtQuick as Q2\nimport \"parts\" as Z\nZ.Part { n: 3; Q2.Item {} }", root)
        console.log("created", cls(made), made.n, made instanceof P.Part, made.content.length)
    }
}
