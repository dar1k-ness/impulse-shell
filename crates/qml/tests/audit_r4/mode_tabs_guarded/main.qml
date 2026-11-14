// RegionSelection.qml reduced to its mode and the Synchronizer that gives it to the toolbar.
import QtQuick
import Qt.labs.synchronizer

Item {
    id: root
    property var selectionMode: 0
    function show(what) {
        console.log(what, "root.selectionMode", root.selectionMode, "toolbar.selectionMode",
                    toolbar.selectionMode, "currentIndex", toolbar.currentIndex)
    }
    ModeToolbar {
        id: toolbar
        Synchronizer on selectionMode { property alias source: root.selectionMode }
    }
    Component.onCompleted: {
        show("loaded:")
        root.selectionMode = 1; show("mode written 1:")
        root.selectionMode = 0; show("mode written 0:")
        toolbar.press(1); show("tab 1 pressed:")
        toolbar.press(0); show("tab 0 pressed:")
        root.selectionMode = 1; show("mode written 1 again:")
        console.log("END")
        Qt.quit()
    }
}
