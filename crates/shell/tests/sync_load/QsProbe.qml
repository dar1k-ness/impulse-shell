import QtQuick
import Quickshell.Io
import Quickshell.Widgets
// The same reading as Probe.qml for what Quickshell adds: FileView and IconImage.
Probe {
    id: more
    readonly property string viewLog: "property var log: []\n"
        + "signal settled()\n"
        + "onLoaded: { log.push('loaded'); settled() }\n"
        + "onLoadFailed: (e) => { log.push('loadFailed=' + e); settled() }\n"
        + "onTextChanged: log.push('text')\n"
        + "onPathChanged: log.push('path')\n"
        + "onLoadedChanged: log.push('loaded=' + loaded)\n"
    readonly property string iconLog: "property var log: []\n"
        + "onStatusChanged: log.push('status=' + status)\n"
        + "onSourceChanged: log.push('source')\n"
        + "onImplicitWidthChanged: log.push('implicitWidth=' + implicitWidth)\n"
    function view(x) { return "loaded " + x.loaded }
    function icon(x) {
        return x.status + " implicit " + x.implicitWidth + "x" + x.implicitHeight + " backer " + x.backer.sourceSize.width + "x"
            + x.backer.sourceSize.height + " " + x.backer.implicitWidth + "x" + x.backer.implicitHeight
    }
    function build(imports, type, body, log) { return Qt.createQmlObject("import QtQuick\n" + imports + "\n" + type + " {\n" + log + body + "\n}", more) }
    function views() {
        // A FileView is read for the last time once its file was read or could not be.
        var V = body => {
            var x = build("import Quickshell.Io", "FileView", body, viewLog)
            hold(x, [x.settled], () => true)
            return x
        }
        var text = x => () => JSON.stringify(x.text())
        for (var block of [false, true]) {
            for (var pre of [true, false]) {
                var how = "blockLoading: " + block + "; preload: " + pre
                var a = V(how + "; path: Qt.resolvedUrl('note.txt')")
                watch("FileView declared, " + how, a, view)
                watch("FileView declared, " + how + ", text()", a, view, text(a))
                let b = V(how)
                watch("FileView written, " + how, b, view, () => { b.path = Qt.resolvedUrl("note.txt") })
                watch("FileView written, " + how + ", text()", b, view, text(b))
                watch("FileView written, " + how + ", text() again", b, view, text(b))
                let c = V(how)
                watch("FileView written, no such file, " + how, c, view, () => { c.path = Qt.resolvedUrl("none.txt") })
                watch("FileView written, no such file, " + how + ", text()", c, view, text(c))
                let d = V(how)
                watch("FileView written, " + how + ", waitForJob()", d, view, () => { d.path = Qt.resolvedUrl("note.txt"); return d.waitForJob() })
                watch("FileView written, " + how + ", text() after waitForJob()", d, view, text(d))
                let e = V(how + "; path: Qt.resolvedUrl('note.txt')")
                watch("FileView declared, " + how + ", reload()", e, view, () => e.reload())
                watch("FileView declared, " + how + ", text() after reload()", e, view, text(e))
            }
        }
        var f = V("blockAllReads: true")
        watch("FileView written, blockAllReads: true", f, view, () => { f.path = Qt.resolvedUrl("note.txt") })
        watch("FileView written, blockAllReads: true, text()", f, view, text(f))
        var g = V("")
        watch("FileView written, a string path", g, view, () => { g.path = Qt.resolvedUrl("note.txt").toString().replace("file://", "") })
    }
    function icons() {
        var I = body => build("import Quickshell.Widgets", "IconImage", body, iconLog)
        watch("IconImage declared", I("implicitSize: 16; source: Qt.resolvedUrl('n.png')"), icon)
        var a = I("implicitSize: 16")
        watch("IconImage with no source", a, icon)
        watch("IconImage written", a, icon, () => { a.source = Qt.resolvedUrl("o.png") })
        watch("IconImage declared asynchronous", I("implicitSize: 16; asynchronous: true; source: Qt.resolvedUrl('p.png')"), icon)
        var b = I("width: 8; height: 8")
        watch("IconImage written, 8x8", b, icon, () => { b.source = Qt.resolvedUrl("t.png") })
        var c = I("implicitSize: 16")
        watch("IconImage written, no such file", c, icon, () => { c.source = Qt.resolvedUrl("none.png") })
    }
    function extra() {
        part("views", views)
        part("icons", icons)
    }
}
