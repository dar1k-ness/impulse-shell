// FileView watchChanges / FolderListModel scenario shared by real Quickshell
// (tools/reference/fswatch/record-qs.sh) and the native runtime (crates/qml/src/fswatch.rs tests).
import QtQuick
import Quickshell
import Quickshell.Io
import Qt.labs.folderlistmodel

ShellRoot {
    id: root
    property string base: "@BASE@"
    property int na: 0
    property int nlink: 0
    property int nnd: 0
    property int nctrl: 0
    property int nself: 0
    property int ntrig: 0
    property int nad: 0
    property int nwr: 0
    property string fl: ""
    function ev(n) { console.log("EV " + Date.now() + " " + n) }

    FileView { path: root.base + "/d/a"; watchChanges: true; onFileChanged: { root.na++; root.ev("a") } }
    FileView { path: root.base + "/d/link"; watchChanges: true; onFileChanged: { root.nlink++; root.ev("link") } }
    FileView { path: root.base + "/nd/x"; watchChanges: true; onFileChanged: { root.nnd++; root.ev("nd") } }
    FileView { path: root.base + "/d/a"; watchChanges: false; onFileChanged: { root.nctrl++; root.ev("ctrl") } }
    FileView { id: sw; path: root.base + "/d/self"; watchChanges: true; onFileChanged: { root.nself++; root.ev("self") } }
    FileView {
        path: root.base + "/d/trigger"; watchChanges: true
        onFileChanged: { root.ntrig++; root.ev("trig"); sw.setText("x" + root.ntrig); ad.v = root.ntrig }
    }
    FileView {
        path: root.base + "/d/ad.json"; watchChanges: true
        onFileChanged: { root.nad++; root.ev("ad"); reload() }
        onAdapterUpdated: { root.nwr++; root.ev("wr"); writeAdapter() }
        JsonAdapter { id: ad; property int v: 0 }
    }

    FolderListModel { id: fm; folder: "file://" + root.base + "/fl"; sortField: FolderListModel.Name }
    Timer {
        interval: 50; running: true; repeat: true
        onTriggered: {
            let s = [];
            for (let i = 0; i < fm.count; i++)
                s.push(fm.get(i, "fileName") + ":" + fm.get(i, "fileSize") + ":" + fm.get(i, "fileModified").getFullYear());
            const v = fm.count + "|" + s.join(",");
            if (v !== root.fl) { root.fl = v; root.ev("fl " + v) }
        }
    }
}
