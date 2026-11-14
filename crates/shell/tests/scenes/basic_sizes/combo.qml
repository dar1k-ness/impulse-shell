import QtQuick
import QtQuick.Controls

// The side paddings of a ComboBox as a script reads them: ComboBox.qml binds both to `padding`,
// and the one on the side of the indicator to its width and the spacing too, until the side is
// written or reset. `write()` moves what the bindings read, `reset()` writes `undefined`,
// `strip()` takes the indicators away.
Item {
    id: root
    width: 600; height: 400

    ComboBox { id: c0 } ComboBox { id: c1; padding: 5; spacing: 4 }
    ComboBox { id: c2; padding: 5; horizontalPadding: 8; spacing: 4 }
    ComboBox { id: c3; padding: 5; spacing: 4; LayoutMirroring.enabled: true }
    ComboBox { id: c4; padding: 5; spacing: 4; indicator: null }
    ComboBox { id: c5; spacing: 4; indicator: Item { width: 10; height: 60 } }
    ComboBox { id: c6; padding: 2; indicator: Item { width: 10; height: 20; visible: false } }
    ComboBox { id: c7; leftPadding: 3 } ComboBox { id: c8; padding: 5; rightPadding: 3 }
    ComboBox { id: c9; horizontalPadding: 8 }
    ComboBox { id: c10; spacing: 1; indicator: Item { implicitWidth: 17; implicitHeight: 9 } }
    ComboBox { id: c11; model: ["one"]; width: 300; padding: 6 }
    ComboBox { id: c12; padding: 5; spacing: 4; leftPadding: 1; rightPadding: 2 }
    ComboBox {
        id: live; padding: 1
        onLeftPaddingChanged: root.nl++
        onRightPaddingChanged: root.nr++
        onAvailableWidthChanged: root.na++
        onImplicitWidthChanged: root.ni++
    }

    property int nl: 0
    property int nr: 0
    property int na: 0
    property int ni: 0
    function zero() { nl = 0; nr = 0; na = 0; ni = 0 }
    function write() {
        zero()
        c0.padding = 7; c1.spacing = 10; c2.rightPadding = 20; c3.LayoutMirroring.enabled = false
        c5.indicator.width = 30; c6.indicator.visible = true; c7.padding = 9; c8.spacing = 6
        c9.padding = 2; c10.indicator.implicitWidth = 23; c11.spacing = 3; c12.padding = 11
        live.spacing = 5; live.padding = 4
    }
    function reset() {
        zero()
        c0.rightPadding = undefined; c1.leftPadding = undefined; c2.rightPadding = undefined
        c3.padding = undefined; c5.spacing = undefined; c7.leftPadding = undefined
        c8.rightPadding = undefined; c9.horizontalPadding = undefined; c12.leftPadding = undefined
        live.rightPadding = undefined
    }
    function strip() {
        zero()
        c0.indicator = null; c1.indicator = null; c2.indicator = null; c3.indicator = null
        c5.indicator = null; c11.indicator = null; live.indicator = null
    }

    readonly property var names: ["implicitWidth", "width", "leftPadding", "rightPadding",
        "availableWidth", "implicitContentWidth", "padding", "horizontalPadding", "spacing"]
    function row(n, o) {
        var s = n + ":"
        for (var i = 0; i < names.length; i++) {
            var v = o[names[i]]
            s += " " + (v === undefined ? "-" : Math.round(v * 100) / 100)
        }
        return s
    }
    function snap() {
        var all = { c0: c0, c1: c1, c2: c2, c3: c3, c4: c4, c5: c5, c6: c6, c7: c7, c8: c8, c9: c9,
            c10: c10, c11: c11, c12: c12, live: live }
        var out = []
        for (var k in all)
            out.push(row(k, all[k]))
        out.push("signals: " + [nl, nr, na, ni].join(" "))
        return out.join("\n")
    }
}
