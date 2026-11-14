import QtQuick
import QtQuick.Controls
import Quickshell
import "sub"
Scope {
    id: root
    Text { id: t }
    TextArea { id: ta }
    TextEdit { id: te }
    Inner { id: inn }
    Sub { id: sb }
    Loader { id: byName; source: "Inner.qml" }
    Loader { id: byFile; source: "file://DIR/out/Out.qml" }
    Loader { id: byPath; source: "DIR/out/Out.qml" }
    function report() {
        return [
            "Probe.qml Text " + t.baseUrl,
            "Probe.qml TextArea " + ta.baseUrl,
            "Probe.qml TextEdit " + te.baseUrl,
            "Inner.qml root " + inn.baseUrl,
            "sub/Sub.qml root " + sb.baseUrl,
            "sub/Sub.qml Text " + sb.inner.baseUrl,
            "sub/Sub.qml TextArea " + sb.area.baseUrl,
            "Loader Inner.qml root " + byName.item.baseUrl,
            "Loader file: root " + byFile.item.baseUrl,
            "Loader file: Text " + byFile.item.inner.baseUrl,
            "Loader file: TextArea " + byFile.item.area.baseUrl,
            "Loader path root " + byPath.item.baseUrl,
            "Loader path Text " + byPath.item.inner.baseUrl,
            "Loader path TextArea " + byPath.item.area.baseUrl,
        ]
    }
    function reportText() { return report().join("\n") }
}
