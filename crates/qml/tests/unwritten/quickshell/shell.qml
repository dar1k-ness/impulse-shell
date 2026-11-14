import QtQuick
import Quickshell
import Quickshell.Services.SystemTray
import Quickshell.Hyprland

Scope {
    id: root
    objectName: "root"
    function probe(label, o, name, call) {
        let line = label + ": typeof " + typeof o[name];
        try {
            line += ", returns " + call(o);
        } catch (e) {
            line += ", throws " + e;
        }
        console.log(line);
    }
    PopupWindow {
        id: popup
    }
    Component.onCompleted: {
        probe("PopupAnchor.updateAnchor", popup.anchor, "updateAnchor", o => o.updateAnchor());
        probe("ObjectModel.indexOf", SystemTray.items, "indexOf", o => o.indexOf(root));
        probe("ObjectModel.indexOf no args", SystemTray.items, "indexOf", o => o.indexOf());
        probe("ObjectModel.sort", SystemTray.items, "sort", o => o.sort(0));
        probe("ObjectModel.submit", SystemTray.items, "submit", o => o.submit());
        probe("ObjectModel.match", SystemTray.items, "match", o => o.match(null, 0, "x").length);
        probe("ObjectModel.match no args", SystemTray.items, "match", o => o.match());
        probe("ObjectModel.sort no args", SystemTray.items, "sort", o => o.sort());
        console.log("done");
        Qt.quit();
    }
}
