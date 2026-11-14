import QtQuick
import QtQuick.Controls

// A ProgressBar has the size of the style of the file that names it: Basic's here (200x6, its
// content 116x6), Material's in MaterialBar.qml (200x4, its content 0x4), whichever file uses
// that one. `strip()` takes parts away.
Item {
    id: root
    width: 600; height: 400

    component Inline: MaterialBar { topPadding: 3 }

    ProgressBar { id: b0 } ProgressBar { id: b1; background: null } ProgressBar { id: b2; contentItem: null }
    ProgressBar { id: b3; padding: 5; topInset: -2; leftInset: -30 }
    MaterialBar { id: m0 } MaterialBar { id: m1; background: null } MaterialBar { id: m2; contentItem: null }
    MaterialBar { id: m3; padding: 5; topInset: -2; leftInset: -30 }
    MaterialBar { id: m4; background: Rectangle { implicitWidth: 50; implicitHeight: 9 } }
    MaterialBar { id: m5; width: 300 }
    NoTrackBar { id: n0 } NoTrackBar { id: n1; width: 300 }
    Inline { id: i0 } Inline { id: i1; background: null }
    Column { id: col; MaterialBar { id: c0 } NoTrackBar { id: c1 } ProgressBar { id: c2 } }

    function strip() { b0.background = null; m0.background = null; m5.contentItem = null; i0.background = null }

    readonly property var names: ["implicitWidth", "implicitHeight", "width", "height",
        "implicitBackgroundWidth", "implicitBackgroundHeight", "implicitContentWidth",
        "implicitContentHeight", "baselineOffset"]
    function row(n, o) {
        var s = n + ":"
        for (var i = 0; i < names.length; i++) {
            var v = o[names[i]]
            s += " " + (v === undefined ? "-" : Math.round(v * 100) / 100)
        }
        return s
    }
    function snap() {
        var all = { b0: b0, b1: b1, b2: b2, b3: b3, m0: m0, m1: m1, m2: m2, m3: m3, m4: m4, m5: m5,
            n0: n0, n1: n1, i0: i0, i1: i1, col: col, c0: c0, c1: c1, c2: c2 }
        var out = []
        for (var k in all)
            out.push(row(k, all[k]))
        return out.join("\n")
    }
}
