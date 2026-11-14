// The lines of `crates/qml/src/qt_palette.tsv`, in `result`: each colour of the palette of an
// item (the application's) and of a control (the Basic style's) nothing set, by group.
import QtQuick
import QtQuick.Controls

Item {
    id: root
    property string result
    Control { id: control }
    Component.onCompleted: {
        var groups = ["active", "inactive", "disabled"]
        var lines = []
        for (var role in root.palette.active) {
            if (typeof root.palette.active[role] !== "object" || role === "parent")
                continue
            var line = [role]
            for (var o of [root, control])
                for (var g of groups)
                    line.push(String(o.palette[g][role]))
            lines.push(line.join("\t"))
        }
        result = lines.join("\n")
    }
}
