import QtQuick
import QtQuick.Controls

// The implicit width TextArea.qml and TextField.qml of the Basic style bind: the wider of
// the text as it is laid out (`contentWidth`: the widest line after the wrap) and the
// placeholder, plus the paddings; a TextField with a background has the background's.
// `snap()` gives a line per field; `type()`, `narrow()` and `clear()` change them in place
// and count the `implicitWidthChanged` and `contentWidthChanged` of `live`.
Item {
    id: root
    width: 600; height: 400
    property int niw: 0
    property int nih: 0
    property int ncw: 0
    property int nch: 0
    readonly property string words: "The quick brown fox jumps over the lazy dog and keeps running through the long meadow"

    // no width: nothing to wrap at
    TextArea { id: a0 } TextArea { id: a1; text: words } TextArea { id: a2; text: words; wrapMode: TextArea.Wrap }
    TextArea { id: a3; text: "one\ntwo two\nthree" }
    // a width and a wrap: the widest line that is left
    TextArea { id: a4; text: words; wrapMode: TextArea.Wrap; width: 200 }
    TextArea { id: a5; text: words; wrapMode: TextArea.Wrap; width: 200; padding: 14 }
    TextArea { id: a6; text: words; wrapMode: TextArea.Wrap; width: 200; leftPadding: 30; rightPadding: 1 }
    TextArea { id: a7; text: words; wrapMode: TextArea.Wrap; width: 200; font.pixelSize: 30 }
    TextArea { id: a8; text: words; wrapMode: TextArea.WordWrap; width: 120 }
    TextArea { id: a9; text: "one two three four five six seven eight nine ten"; wrapMode: TextArea.Wrap; width: 100 }
    TextArea { id: a10; text: "one two\nthree four five six seven\neight"; wrapMode: TextArea.Wrap; width: 100 }
    // a width and no wrap
    TextArea { id: a11; text: words; width: 200 }
    // the placeholder is not wrapped for the width, and is for the height
    TextArea { id: a12; placeholderText: "Type here" } TextArea { id: a13; placeholderText: "Type here"; wrapMode: TextArea.Wrap; width: 200 }
    TextArea { id: a14; placeholderText: words; wrapMode: TextArea.Wrap; width: 200 }
    TextArea { id: a15; text: "Hi"; placeholderText: "Type here please"; wrapMode: TextArea.Wrap; width: 200 }
    TextArea { id: a16; text: words; placeholderText: "Type here please"; wrapMode: TextArea.Wrap; width: 200 }
    // a background of its own
    TextArea { id: a17; text: words; wrapMode: TextArea.Wrap; width: 200; background: Rectangle { implicitWidth: 50; implicitHeight: 60 } }
    TextArea { id: a18; text: words; wrapMode: TextArea.Wrap; width: 200; background: Rectangle { implicitWidth: 250; implicitHeight: 10 } }
    TextArea { id: a19; text: words; wrapMode: TextArea.Wrap; width: 200; background: null }
    // in a ScrollView the area is as wide as the view
    ScrollView { id: v0; width: 300; height: 100; TextArea { id: s0; text: words; wrapMode: TextArea.Wrap; padding: 14 } }
    ScrollView { id: v1; width: 300; height: 100; TextArea { id: s1; text: words; padding: 14 } }
    ScrollView { id: v2; width: 300; height: 100; TextArea { id: s2; placeholderText: "Type here"; wrapMode: TextArea.Wrap } }

    TextField { id: f0 } TextField { id: f1; text: words } TextField { id: f2; placeholderText: words }
    TextField { id: f3; text: words; background: null } TextField { id: f4; placeholderText: words; background: null }
    TextField { id: f5; text: "Hi"; placeholderText: "Type here please"; background: null }
    TextField { id: f6; text: words; placeholderText: "Type here please"; background: null }
    TextField { id: f7; text: words; background: null; width: 100 }
    TextField { id: f8; text: words; background: null; padding: 14; font.pixelSize: 30 }
    TextField { id: f9; text: words; background: Rectangle { implicitWidth: 50; implicitHeight: 60 } }
    TextField { id: f10; text: words; background: Item {} }

    TextArea {
        id: live; wrapMode: TextArea.Wrap; width: 200; placeholderText: "Type here"
        onImplicitWidthChanged: niw++
        onImplicitHeightChanged: nih++
        onContentWidthChanged: ncw++
        onContentHeightChanged: nch++
    }
    TextField { id: livef; background: null; placeholderText: "Type here" }
    ScrollView { id: livev; width: 300; height: 100; TextArea { id: lives; wrapMode: TextArea.Wrap; padding: 14 } }

    function zero() { niw = 0; nih = 0; ncw = 0; nch = 0 }
    function type() {
        zero()
        live.text = words; livef.text = words; lives.text = words
    }
    function narrow() {
        zero()
        live.width = 120; livef.width = 120; livev.width = 150
        a4.width = 90; a5.wrapMode = TextArea.NoWrap; a11.wrapMode = TextArea.Wrap; a14.width = 400
    }
    function clear() {
        zero()
        live.text = ""; livef.text = "Hi"; lives.text = "Hi"
        a4.text = "Hi"; a16.text = ""
    }

    readonly property var names: ["implicitWidth", "implicitHeight", "width", "height", "contentWidth",
        "contentHeight", "lineCount", "implicitBackgroundWidth", "implicitBackgroundHeight",
        "leftPadding", "rightPadding"]
    function row(n, o) {
        var s = n + ":"
        for (var i = 0; i < names.length; i++) {
            var v = o[names[i]]
            s += " " + (v === undefined ? "-" : Math.round(v * 100) / 100)
        }
        return s
    }
    function snap() {
        var all = {
            a0: a0, a1: a1, a2: a2, a3: a3, a4: a4, a5: a5, a6: a6, a7: a7, a8: a8, a9: a9, a10: a10,
            a11: a11, a12: a12, a13: a13, a14: a14, a15: a15, a16: a16, a17: a17, a18: a18, a19: a19,
            s0: s0, s1: s1, s2: s2, v0: v0, v1: v1, v2: v2,
            f0: f0, f1: f1, f2: f2, f3: f3, f4: f4, f5: f5, f6: f6, f7: f7, f8: f8, f9: f9, f10: f10,
            live: live, livef: livef, lives: lives, livev: livev
        }
        var out = []
        for (var k in all)
            out.push(row(k, all[k]))
        out.push("signals: " + [niw, nih, ncw, nch].join(" "))
        return out.join("\n")
    }
}
