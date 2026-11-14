import QtQuick
import QtQuick.Controls

// The size the Basic style gives a control from the parts the document did not set
// (background, contentItem, indicator, handle), and the control's baseline. `snap()` gives a
// line per control; `grow()`, `restyle()`, `strip()` and `shrink()` change them in place and
// count the `implicit*Changed` signals of `live`.
Item {
    id: root
    width: 400; height: 300
    property int niw: 0
    property int nih: 0
    property int nicw: 0
    property int nich: 0
    property int nbase: 0
    property int nw: 0

    // nothing set
    Button { id: b0 } TabButton { id: tb0 } ItemDelegate { id: id0 } MenuItem { id: mi0 }
    RadioButton { id: rb0 } Switch { id: sw0 } CheckBox { id: cb0 } Slider { id: sl0 }
    ProgressBar { id: pb0 } ScrollBar { id: sb0 } ComboBox { id: co0 } TextField { id: tf0 }
    TextArea { id: ta0 } DayOfWeekRow { id: dw0 } RoundButton { id: ro0 } ToolButton { id: to0 }
    Popup { id: po0 } Menu { id: me0 } ToolTip { id: tt0 }

    // a text
    Button { id: b1; text: "Hello" } TabButton { id: tb1; text: "Hello" }
    ItemDelegate { id: id1; text: "Hello" } MenuItem { id: mi1; text: "Hello" }
    RadioButton { id: rb1; text: "Hello" } Switch { id: sw1; text: "Hello" }
    CheckBox { id: cb1; text: "Hello" } TextField { id: tf1; text: "Hello" }
    TextField { id: tf2; placeholderText: "Hello there" } TextArea { id: ta1; text: "Hello\nthere" }
    ToolTip { id: tt1; text: "Hello" } ComboBox { id: co1; model: ["one", "two hundred"] }
    Button { id: b2; text: "Hello\nthere" }
    Button { id: b3; text: "A rather long text of a button that is wider than its background" }

    // a font size
    Button { id: b4; text: "Hello"; font.pixelSize: 30 } TabButton { id: tb4; text: "Hello"; font.pixelSize: 30 }
    RadioButton { id: rb4; text: "Hello"; font.pixelSize: 30 } Switch { id: sw4; text: "Hello"; font.pixelSize: 30 }
    RadioButton { id: rb5; font.pixelSize: 40 } MenuItem { id: mi4; text: "Hello"; font.pixelSize: 30 }
    ItemDelegate { id: id4; text: "Hello"; font.pixelSize: 40 } TextField { id: tf4; text: "Hello"; font.pixelSize: 40 }
    ComboBox { id: co4; model: ["one"]; font.pixelSize: 40 } ToolTip { id: tt4; text: "Hello"; font.pixelSize: 30 }
    DayOfWeekRow { id: dw4; font.pixelSize: 20 }

    // spacing
    RadioButton { id: rb6; text: "Hello"; spacing: 20 } Switch { id: sw6; spacing: 20 }
    ComboBox { id: co6; spacing: 20 } Button { id: b6; text: "Hello"; spacing: 20 }
    DayOfWeekRow { id: dw6; spacing: 0 }

    // paddings
    Button { id: b7; padding: 20 } Button { id: b8; text: "Hello"; leftPadding: 60; topPadding: 30 }
    Button { id: b9; horizontalPadding: 70; verticalPadding: 25 }
    RadioButton { id: rb7; padding: 0 } Switch { id: sw7; topPadding: 20 } Slider { id: sl7; padding: 30 }
    Slider { id: sl8; leftPadding: 100; rightPadding: 100 } ProgressBar { id: pb7; padding: 5 }
    ScrollBar { id: sb7; padding: 0 } ComboBox { id: co7; padding: 10 } ComboBox { id: co8; rightPadding: 3 }
    TextField { id: tf7; padding: 20 } TextField { id: tf8; leftPadding: 0 } TabButton { id: tb7; padding: 30 }
    ItemDelegate { id: id7; padding: 0 } MenuItem { id: mi7; padding: 30 } Popup { id: po7; padding: 3 }
    ToolTip { id: tt7; text: "Hello"; padding: 0 } DayOfWeekRow { id: dw7; leftPadding: 10; topPadding: 1 }

    // insets
    Button { id: b10; topInset: 1; leftInset: 2; rightInset: 3; bottomInset: 4 }
    Button { id: b11; topInset: -1; leftInset: -2; rightInset: -3; bottomInset: -4 }
    Slider { id: sl10; topInset: 1; leftInset: 2; rightInset: 3; bottomInset: 4 }
    ProgressBar { id: pb10; topInset: 1; leftInset: 2; rightInset: 3; bottomInset: 4 }
    ComboBox { id: co10; topInset: 1; leftInset: 2; rightInset: 3; bottomInset: 4 }
    TextField { id: tf10; topInset: 1; leftInset: 2; rightInset: 3; bottomInset: 4 }
    MenuItem { id: mi10; topInset: 1; leftInset: 2; rightInset: 3; bottomInset: 4 }
    TabButton { id: tb10; topInset: 1; leftInset: 2; rightInset: 3; bottomInset: 4 }
    Menu { id: me10; topInset: 1; leftInset: 2; rightInset: 3; bottomInset: 4 }

    // a part of the document's own, and none
    Button { id: b12; background: Rectangle { implicitWidth: 50; implicitHeight: 20 } }
    Button { id: b13; background: null } Button { id: b14; text: "Hello"; background: null }
    Button { id: b15; contentItem: Item { implicitWidth: 30; implicitHeight: 10 } }
    Button { id: b16; contentItem: null } Button { id: b17; text: "Hello"; contentItem: null; background: null }
    RadioButton { id: rb12; indicator: null } RadioButton { id: rb13; text: "Hello"; indicator: null }
    RadioButton { id: rb14; indicator: Item { implicitWidth: 10; implicitHeight: 50 } }
    RadioButton { id: rb15; text: "Hello"; indicator: Item { width: 10; height: 50 } }
    RadioButton { id: rb16; contentItem: null } Switch { id: sw12; indicator: null }
    Switch { id: sw13; contentItem: Text { text: "Hello" } }
    Slider { id: sl12; handle: null } Slider { id: sl13; background: null }
    Slider { id: sl14; handle: Item { implicitWidth: 10; implicitHeight: 60 } }
    ProgressBar { id: pb12; background: null } ProgressBar { id: pb13; contentItem: null }
    ProgressBar { id: pb14; contentItem: Item { implicitWidth: 300; implicitHeight: 2 } }
    ScrollBar { id: sb12; contentItem: null } ScrollBar { id: sb13; contentItem: Item { implicitWidth: 20; implicitHeight: 30 } }
    ComboBox { id: co12; indicator: null } ComboBox { id: co13; background: null }
    ComboBox { id: co14; contentItem: null } ComboBox { id: co15; indicator: Item { width: 10; height: 60 } }
    ComboBox { id: co16; contentItem: Text { text: "Hello" } }
    TextField { id: tf12; background: null } TextField { id: tf13; text: "Hello"; background: null }
    TextArea { id: ta12; background: Rectangle { implicitWidth: 50; implicitHeight: 60 } }
    TabButton { id: tb12; background: null } ItemDelegate { id: id12; background: null }
    MenuItem { id: mi12; background: null } MenuItem { id: mi13; text: "Hello"; contentItem: null }
    Menu { id: me12; background: null } ToolTip { id: tt12; text: "Hello"; contentItem: null }
    Popup { id: po12; contentItem: Item { implicitWidth: 30; implicitHeight: 10 } }
    Popup { id: po13; background: Rectangle { implicitWidth: 50; implicitHeight: 60 } }

    // the state a part's size follows
    Slider { id: sl15; orientation: Qt.Vertical } ScrollBar { id: sb15; orientation: Qt.Horizontal }
    ScrollBar { id: sb16; interactive: false } ProgressBar { id: pb15; indeterminate: true }
    MenuItem { id: mi15; checkable: true } MenuItem { id: mi16; text: "Hello"; checkable: true; checked: true }
    Button { id: b18; flat: true } Button { id: b19; text: "Hello"; display: AbstractButton.IconOnly }
    RadioButton { id: rb18; text: "Hello"; LayoutMirroring.enabled: true }
    ComboBox { id: co18; LayoutMirroring.enabled: true } ComboBox { id: co19; editable: true }
    ComboBox { id: co20; flat: true } Switch { id: sw18; checked: true }

    // a size of the document's own
    Button { id: b20; width: 30; height: 90 } Button { id: b21; implicitWidth: 30; implicitHeight: 90 }
    RadioButton { id: rb20; text: "Hello"; height: 90 } Switch { id: sw20; text: "Hello"; width: 30; height: 10 }
    ComboBox { id: co21; width: 300; height: 90 } TextField { id: tf20; text: "Hello"; height: 90 }
    Slider { id: sl20; width: 50; height: 90 } ProgressBar { id: pb20; height: 30 }
    TabButton { id: tb20; text: "Hello"; height: 90 } ItemDelegate { id: id20; text: "Hello"; height: 90 }

    // changed in place
    Button {
        id: live
        onImplicitWidthChanged: root.niw++
        onImplicitHeightChanged: root.nih++
        onImplicitContentWidthChanged: root.nicw++
        onImplicitContentHeightChanged: root.nich++
        onBaselineOffsetChanged: root.nbase++
        onWidthChanged: root.nw++
    }
    RadioButton { id: liver } Switch { id: lives } TabButton { id: livet } ItemDelegate { id: lived }
    MenuItem { id: livem } TextField { id: livef } ToolTip { id: livett } ComboBox { id: livec; model: ["one"] }
    Slider { id: livesl } ProgressBar { id: livep } ScrollBar { id: livesb }

    function zero() { niw = 0; nih = 0; nicw = 0; nich = 0; nbase = 0; nw = 0 }
    function grow() {
        zero()
        live.text = "A rather long text of a button that is wider than its background"
        liver.text = "Hello"; lives.text = "Hello"; livet.text = "Hello"; lived.text = "Hello"
        livem.text = "Hello"; livef.text = "Hello"; livett.text = "Hello"; livec.model = ["one", "two hundred and two"]
        livesl.orientation = Qt.Vertical; livep.indeterminate = true; livesb.interactive = false
    }
    function restyle() {
        zero()
        live.font.pixelSize = 50
        liver.font.pixelSize = 40; lives.spacing = 30; livet.padding = 20; lived.topPadding = 40
        livem.leftInset = 5; livem.topInset = -50; livef.font.pixelSize = 40; livett.padding = 1
        livec.spacing = 9; livec.font.pixelSize = 40; livesl.padding = 0; livep.bottomInset = 7; livesb.padding = 10
    }
    function strip() {
        zero()
        live.background = null
        liver.indicator = null; lives.contentItem = null; livet.background = null; lived.contentItem = null
        livem.background = null; livef.background = null; livec.indicator = null
        livesl.handle = null; livep.contentItem = null; livesb.contentItem = null
    }
    function shrink() {
        zero()
        live.text = ""
        liver.text = ""; lives.spacing = 0; livet.padding = 2; lived.topPadding = 0
        livem.leftInset = 0; livem.topInset = 0; livef.text = ""; livett.text = ""
        livec.model = []; livesl.padding = 4; livep.bottomInset = 0; livesb.padding = 1
    }

    readonly property var names: ["implicitWidth", "implicitHeight", "width", "height", "baselineOffset",
        "implicitBackgroundWidth", "implicitBackgroundHeight", "implicitContentWidth", "implicitContentHeight",
        "implicitIndicatorWidth", "implicitIndicatorHeight", "implicitHandleWidth", "implicitHandleHeight",
        "availableWidth", "availableHeight", "leftPadding", "rightPadding", "topPadding", "bottomPadding"]
    function row(n, o) {
        var s = n + ":"
        // What a script reads of the side paddings ComboBox.qml binds is not what this is
        // about; the room they leave (availableWidth) is.
        var combo = n.indexOf("co") === 0 || n === "livec"
        for (var i = 0; i < names.length; i++) {
            var v = combo && /^(left|right)Padding$/.test(names[i]) ? undefined : o[names[i]]
            s += " " + (v === undefined ? "-" : Math.round(v * 100) / 100)
        }
        return s
    }
    function snap() {
        var all = {
            b0: b0, tb0: tb0, id0: id0, mi0: mi0, rb0: rb0, sw0: sw0, cb0: cb0, sl0: sl0, pb0: pb0, sb0: sb0,
            co0: co0, tf0: tf0, ta0: ta0, dw0: dw0, ro0: ro0, to0: to0, po0: po0, me0: me0, tt0: tt0,
            b1: b1, tb1: tb1, id1: id1, mi1: mi1, rb1: rb1, sw1: sw1, cb1: cb1, tf1: tf1, tf2: tf2, ta1: ta1,
            tt1: tt1, co1: co1, b2: b2, b3: b3,
            b4: b4, tb4: tb4, rb4: rb4, sw4: sw4, rb5: rb5, mi4: mi4, id4: id4, tf4: tf4, co4: co4, tt4: tt4, dw4: dw4,
            rb6: rb6, sw6: sw6, co6: co6, b6: b6, dw6: dw6,
            b7: b7, b8: b8, b9: b9, rb7: rb7, sw7: sw7, sl7: sl7, sl8: sl8, pb7: pb7, sb7: sb7, co7: co7, co8: co8,
            tf7: tf7, tf8: tf8, tb7: tb7, id7: id7, mi7: mi7, po7: po7, tt7: tt7, dw7: dw7,
            b10: b10, b11: b11, sl10: sl10, pb10: pb10, co10: co10, tf10: tf10, mi10: mi10, tb10: tb10, me10: me10,
            b12: b12, b13: b13, b14: b14, b15: b15, b16: b16, b17: b17, rb12: rb12, rb13: rb13, rb14: rb14,
            rb15: rb15, rb16: rb16, sw12: sw12, sw13: sw13, sl12: sl12, sl13: sl13, sl14: sl14, pb12: pb12,
            pb13: pb13, pb14: pb14, sb12: sb12, sb13: sb13, co12: co12, co13: co13, co14: co14, co15: co15,
            co16: co16, tf12: tf12, tf13: tf13, ta12: ta12, tb12: tb12, id12: id12, mi12: mi12, mi13: mi13,
            me12: me12, tt12: tt12, po12: po12, po13: po13,
            sl15: sl15, sb15: sb15, sb16: sb16, pb15: pb15, mi15: mi15, mi16: mi16, b18: b18, b19: b19,
            rb18: rb18, co18: co18, co19: co19, co20: co20, sw18: sw18,
            b20: b20, b21: b21, rb20: rb20, sw20: sw20, co21: co21, tf20: tf20, sl20: sl20, pb20: pb20,
            tb20: tb20, id20: id20,
            live: live, liver: liver, lives: lives, livet: livet, lived: lived, livem: livem, livef: livef,
            livett: livett, livec: livec, livesl: livesl, livep: livep, livesb: livesb
        }
        var out = []
        for (var k in all)
            out.push(row(k, all[k]))
        out.push("signals: " + [niw, nih, nicw, nich, nbase, nw].join(" "))
        return out.join("\n")
    }
}
