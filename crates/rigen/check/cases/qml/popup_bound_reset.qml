// A side margin of a popup that a binding resets by giving `undefined` follows `margins` as
// after `= undefined` from a script: what the type gives them (6 for ToolTip, 0 for Menu),
// -1 for a Popup that has none. The binding is one a script made, or one of the document.
// qt: leftMargin 6,9,6,9,6,9,6 | rightMargin 6,9,6,9,6,9,6 | topMargin 6,9,6,9,6,9,6 | bottomMargin 6,9,6,9,6,9,6 | leftMargin 0,9,0,9,0,9,0 | rightMargin 0,9,0,9,0,9,0 | topMargin 0,9,0,9,0,9,0 | bottomMargin 0,9,0,9,0,9,0 | leftMargin -1,9,-1,9,-1,9,-1 | rightMargin -1,9,-1,9,-1,9,-1 | topMargin -1,9,-1,9,-1,9,-1 | bottomMargin -1,9,-1,9,-1,9,-1 | leftMargin 11,9,11,9,11,9,11 | rightMargin 11,9,11,9,11,9,11 | topMargin 11,9,11,9,11,9,11 | bottomMargin 11,9,11,9,11,9,11 | declared 9,6,9 | margins 6,2,-1,2,-1,2,-1 | margins 0,2,-1,2,-1,2,-1 | margins -1,2,-1,2,-1,2,-1 | margins 11,2,-1,2,-1,2,-1
import QtQuick
import QtQuick.Controls
Item {
    id: root
    property bool flag: true
    ToolTip { id: tip }
    Menu { id: menu }
    Popup { id: popup }
    Popup { id: wide; margins: 11 }
    ToolTip { id: declared; leftMargin: root.flag ? 9 : undefined }
    property string result
    function probe(o, path, v) {
        var out = [o[path]]
        o[path] = v; out.push(o[path])
        o[path] = undefined; out.push(o[path])
        root.flag = true
        o[path] = Qt.binding(() => root.flag ? v : undefined); out.push(o[path])
        root.flag = false; out.push(o[path])
        root.flag = true; out.push(o[path])
        o[path] = undefined; out.push(o[path])
        return path + " " + out.join(",")
    }
    Component.onCompleted: {
        var l = []
        for (var o of [tip, menu, popup, wide])
            for (var s of ["left", "right", "top", "bottom"])
                l.push(probe(o, s + "Margin", 9))
        var d = [declared.leftMargin]
        flag = false; d.push(declared.leftMargin)
        flag = true; d.push(declared.leftMargin)
        l.push("declared " + d.join(","))
        for (var o of [tip, menu, popup, wide])
            l.push(probe(o, "margins", 2))
        result = l.join(" | ")
    }
}
