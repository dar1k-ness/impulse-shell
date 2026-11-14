import QtQuick
import "sub"
Item {
    id: root
    MyLoader { id: l1; source: "B.qml" }
    Loader { id: l2; source: "my%20file.qml" }
    Loader { id: l3; source: "my file.qml" }
    Loader { id: l4; source: "./sub/../B.qml?x=1" }
    Loader { id: l5; source: Qt.resolvedUrl("sub/B.qml") }
    Loader { id: l6; source: "B.qml#frag" }
    function show(l) { return l.status + ":" + (l.item ? l.item.where : "-") }
    Component.onCompleted: {
        console.log("loaders", show(l1), show(l2), show(l3), show(l4), show(l5), show(l6));
        console.log("END"); Qt.quit();
    }
}
