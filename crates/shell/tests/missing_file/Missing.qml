import QtQuick
// An asynchronous Image whose file is not there, and what loads once the file is: the same
// Image left alone, the same Image given the same source again, and a new Image.
Item {
    id: root
    property string dir
    property string report: ""
    property var out: []
    property string want: ""
    signal finished()
    readonly property string file: dir + "/appears.png"
    function row(what, x) { out.push(what + " | " + x.status + " " + x.implicitWidth + "x" + x.implicitHeight) }
    // `then` runs once `x` is not loading, a turn of the event loop later at the earliest.
    function settled(x, then) {
        Qt.callLater(() => {
            if (x.status !== Image.Loading) {
                then()
                return
            }
            var f = () => {
                if (x.status === Image.Loading)
                    return
                x.statusChanged.disconnect(f)
                then()
            }
            x.statusChanged.connect(f)
        })
    }
    Image { id: a; asynchronous: true }
    Component { id: fresh; Image { asynchronous: true } }
    property var b
    Component.onCompleted: {
        a.source = file
        row("no file: same call", a)
        settled(a, () => {
            row("no file: later", a)
            want = "appears.png"
        })
    }
    // The file is there now.
    function fileThere() {
        row("file there, source untouched: same call", a)
        settled(a, () => {
            row("file there, source untouched: later", a)
            a.source = file
            row("the same source written again: same call", a)
            settled(a, () => {
                row("the same source written again: later", a)
                b = fresh.createObject(root, { source: file })
                row("a new Image: same call", b)
                settled(b, () => {
                    row("a new Image: later", b)
                    row("the first Image at the end", a)
                    report = out.join("\n")
                    finished()
                })
            })
        })
    }
}
