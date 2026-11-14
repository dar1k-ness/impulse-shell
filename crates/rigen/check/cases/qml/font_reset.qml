// The font of a control: `font = undefined` leaves no field a set one and writes no value, the
// family nothing set has one name (QGuiApplication::font()), `undefined` written to a field by
// a script is the value of the field's type made of it, and a control, a label and a text
// field take the fields nothing set in them from the control, popup or window they are in.
// qt: Sans Serif,sans-serif,Sans Serif,Sans Serif true,true,true QFont(Serif,9,-1,5,400,0,0,0,0,0,0,0,0,0,0,1,,0,0) QFont(Sans Serif,-1,20,5,400) | fresh QFont(Sans Serif,9,-1,5,400) | written QFont(Serif,-1,21,5,700) | written reset QFont(Sans Serif,9,-1,5,400)12Sans Seriffalse n1 | document QFont(Sans Serif,9,-1,5,400)QFont(Sans Serif,-1,21,5,400) | document QFont(Sans Serif,-1,14,5,400) | again QFont(Sans Serif,9,-1,5,400) | italic QFont(Sans Serif,9,-1,5,400,1,0,0,0,0,0,0,0,0,0,1,,0,0) | declared QFont(Serif,-1,15,5,400)QFont(Sans Serif,9,-1,5,400) n1 | whole QFont(Sans Serif,-1,15,5,400)QFont(Sans Serif,9,-1,5,400) n1 | value QFont(Serif,-1,30,5,400)30 n1 | value reset QFont(Sans Serif,9,-1,5,400)12 n1 | bound QFont(Sans Serif,-1,15,5,400)QFont(Sans Serif,9,-1,5,400)QFont(Sans Serif,-1,16,5,400,1,0,0,0,0,0,0,0,0,0,1,,0,0) n1 | bound again QFont(Sans Serif,-1,15,5,400)QFont(Sans Serif,-1,15,5,400,1,0,0,0,0,0,0,0,0,0,1,,0,0) n1 | side reset QFont(Sans Serif,9,-1,5,400) n1 | no reset throws Cannot assign [undefined] to QFontQFont(Sans Serif,-1,19,5,400) | no reset throws Cannot assign [undefined] to QFontQFont(Sans Serif,-1,19,5,400) | no reset throws Cannot assign [undefined] to QFontQFont(Sans Serif,-1,19,5,400) | family QFont(Serif,9,-1,5,400)Serif>QFont(,9,-1,5,400) | styleName QFont(Sans Serif,9,-1,5,400,0,0,0,0,0,0,0,0,0,0,1,Bold,0,0)Bold>QFont(Sans Serif,9,-1,5,400) | bold QFont(Sans Serif,9,-1,5,700)true>QFont(Sans Serif,9,-1,5,400)false | weight QFont(Sans Serif,9,-1,5,700)700>QFont(Sans Serif,9,-1,5,1)1 | italic QFont(Sans Serif,9,-1,5,400,1,0,0,0,0,0,0,0,0,0,1,,0,0)true>QFont(Sans Serif,9,-1,5,400)false | underline QFont(Sans Serif,9,-1,5,400,0,1,0,0,0,0,0,0,0,0,1,,0,0)true>QFont(Sans Serif,9,-1,5,400)false | overline QFont(Sans Serif,9,-1,5,400)true>QFont(Sans Serif,9,-1,5,400)false | strikeout QFont(Sans Serif,9,-1,5,400,0,0,1,0,0,0,0,0,0,0,1,,0,0)true>QFont(Sans Serif,9,-1,5,400)false | pointSize QFont(Sans Serif,14,-1,5,400)14>QFont(Sans Serif,14,-1,5,400)14 | pixelSize QFont(Sans Serif,-1,21,5,400)21>QFont(Sans Serif,-1,21,5,400)21 | capitalization QFont(Sans Serif,9,-1,5,400,0,0,0,0,0,1,0,0,0,0,1,,0,0)1>QFont(Sans Serif,9,-1,5,400)0 | letterSpacing QFont(Sans Serif,9,-1,5,400,0,0,0,0,0,0,1,2,0,0,1,,0,0)2>QFont(Sans Serif,9,-1,5,400,0,0,0,0,0,0,1,-3.35544e+07,0,0,1,,0,0)-33554432 | wordSpacing QFont(Sans Serif,9,-1,5,400,0,0,0,0,0,0,0,0,3,0,1,,0,0)3>QFont(Sans Serif,9,-1,5,400,0,0,0,0,0,0,0,0,-3.35544e+07,0,1,,0,0)-33554432 | hintingPreference QFont(Sans Serif,9,-1,5,400)1>QFont(Sans Serif,9,-1,5,400)0 | kerning QFont(Sans Serif,9,-1,5,400)false>QFont(Sans Serif,9,-1,5,400)false | preferShaping QFont(Sans Serif,9,-1,5,400,0,0,0,0,0,0,0,0,0,0,4097,,0,0)false>QFont(Sans Serif,9,-1,5,400,0,0,0,0,0,0,0,0,0,0,4097,,0,0)false | contextFontMerging QFont(Sans Serif,9,-1,5,400,0,0,0,0,0,0,0,0,0,0,8193,,0,0)true>QFont(Sans Serif,9,-1,5,400)false | preferTypoLineMetrics QFont(Sans Serif,9,-1,5,400,0,0,0,0,0,0,0,0,0,0,16385,,0,0)true>QFont(Sans Serif,9,-1,5,400)false | tree QFont(Serif,-1,20,5,400)QFont(Serif,-1,20,5,400)QFont(Sans Serif,9,-1,5,400)QFont(Serif,-1,20,5,400,1,0,0,0,0,0,0,0,0,0,1,,0,0)QFont(Serif,-1,20,5,700)QFont(Serif,-1,20,5,700)QFont(Serif,-1,20,5,700)QFont(Sans Serif,-1,19,5,400)QFont(Sans Serif,-1,19,5,400)QFont(Sans Serif,-1,18,5,400)QFont(Sans Serif,-1,18,5,400)QFont(Sans Serif,-1,18,5,400)QFont(Sans Serif,9,-1,5,400) | own QFont(Serif,-1,20,5,400,0,1,0,0,0,0,0,0,0,0,1,,0,0)QFont(Serif,-1,11,5,400,0,1,0,0,0,0,0,0,0,0,1,,0,0)QFont(Sans Serif,9,-1,5,400)QFont(Serif,-1,20,5,400,1,1,0,0,0,0,0,0,0,0,1,,0,0)QFont(Serif,-1,20,5,700,0,1,0,0,0,0,0,0,0,0,1,,0,0)QFont(Sans,-1,20,5,700,0,1,0,0,0,0,0,0,0,0,1,,0,0)QFont(Sans,-1,20,5,700,0,1,0,0,0,0,0,0,0,0,1,,0,0)QFont(Sans Serif,-1,19,5,400)QFont(Sans Serif,-1,19,5,400)QFont(Sans Serif,-1,18,5,400)QFont(Sans Serif,-1,18,5,400)QFont(Sans Serif,-1,18,5,400)QFont(Sans Serif,9,-1,5,400) | own reset QFont(Serif,-1,20,5,400,0,1,0,0,0,0,0,0,0,0,1,,0,0)QFont(Serif,-1,20,5,400,0,1,0,0,0,0,0,0,0,0,1,,0,0)QFont(Sans Serif,9,-1,5,400)QFont(Serif,-1,20,5,400,1,1,0,0,0,0,0,0,0,0,1,,0,0)QFont(Serif,-1,20,5,700,0,1,0,0,0,0,0,0,0,0,1,,0,0)QFont(Serif,-1,20,5,700,0,1,0,0,0,0,0,0,0,0,1,,0,0)QFont(Serif,-1,20,5,700,0,1,0,0,0,0,0,0,0,0,1,,0,0)QFont(Sans Serif,-1,19,5,400)QFont(Sans Serif,-1,19,5,400)QFont(Sans Serif,-1,18,5,400)QFont(Sans Serif,-1,18,5,400)QFont(Sans Serif,-1,18,5,400)QFont(Sans Serif,9,-1,5,400) | reset QFont(Sans Serif,9,-1,5,400)QFont(Sans Serif,9,-1,5,400)QFont(Sans Serif,9,-1,5,400)QFont(Sans Serif,9,-1,5,400,1,0,0,0,0,0,0,0,0,0,1,,0,0)QFont(Sans Serif,9,-1,5,700)QFont(Sans Serif,9,-1,5,700)QFont(Sans Serif,9,-1,5,700)QFont(Sans Serif,9,-1,5,400)QFont(Sans Serif,9,-1,5,400)QFont(Sans Serif,9,-1,5,400)QFont(Sans Serif,9,-1,5,400)QFont(Sans Serif,9,-1,5,400)QFont(Sans Serif,9,-1,5,400) | set QFont(Sans Serif,-1,17,5,400)QFont(Sans Serif,-1,17,5,400)QFont(Sans Serif,9,-1,5,400)QFont(Sans Serif,-1,17,5,400,1,0,0,0,0,0,0,0,0,0,1,,0,0)QFont(Sans Serif,-1,17,5,700)QFont(Sans Serif,-1,17,5,700)QFont(Sans Serif,-1,17,5,700)QFont(Sans Serif,-1,17,5,400)QFont(Sans Serif,-1,17,5,400)QFont(Sans Serif,-1,17,5,400)QFont(Sans Serif,-1,17,5,400)QFont(Sans Serif,-1,17,5,400)QFont(Sans Serif,9,-1,5,400) | moved QFont(Sans Serif,-1,17,5,400) n1 | moved QFont(Sans Serif,-1,30,5,400) n1 | moved QFont(Sans Serif,-1,17,5,700) n1 | moved QFont(Sans Serif,-1,17,5,700) n2 | moved QFont(Sans Serif,9,-1,5,400) n1 | moved QFont(Sans Serif,-1,17,5,400) n1 | given QFont(Sans Serif,-1,13,5,400) n1
import QtQuick
import QtQuick.Controls
Item {
    id: root
    property int n: 0
    property bool flag: true
    Text { id: named; font.family: "Sans Serif" }
    Text { id: generic; font.family: "sans-serif" }
    Text { id: plain }
    Text { id: made; font: Qt.font({pixelSize: 20}) }
    Text { id: text }
    Label { id: label }
    TextField { id: field }
    Button { id: fresh; onFontChanged: root.n++ }
    Button { id: written; onFontChanged: root.n++ }
    Button { id: declared; font.pixelSize: 15; font.family: "Serif"; onFontChanged: root.n++ }
    Button { id: none; font: undefined; onFontChanged: root.n++ }
    Pane { font.pixelSize: 21; Button { id: noneInPane; font: undefined } }
    Button { id: whole; font: Qt.font({pixelSize: 15}); onFontChanged: root.n++ }
    Button { id: bound; font: root.flag ? Qt.font({pixelSize: 15}) : undefined; onFontChanged: root.n++ }
    Button { id: side; font.pixelSize: root.flag ? 15 : 16; font.italic: true }
    Button { id: fields }
    Pane {
        id: pane
        font.pixelSize: 20; font.family: "Serif"
        Button { id: inPane }
        Text { id: textInPane }
        Label { id: labelInPane; font.italic: true }
        Frame {
            id: frame
            font.bold: true
            Button { id: deep; Label { id: deeper } }
        }
    }
    Popup {
        id: popup
        font.pixelSize: 19
        Button { id: inPopup }
    }
    ApplicationWindow {
        id: app
        font.pixelSize: 18
        Button { id: inApp }
        Item { Label { id: underItem } }
    }
    Pane { id: other; font.pixelSize: 30 }
    Button { id: loose; onFontChanged: root.n++ }
    property string result
    function show(o) { return String(o.font).replace(/,0,0,0,0,0,0,0,0,0,0,1,,0,0\)$/, ")") }
    function step(name, f) {
        var c = n, r
        try { r = f() } catch (e) { r = "throws " + e.message }
        return name + " " + r + (n != c ? " n" + (n - c) : "")
    }
    Component.onCompleted: {
        var l = []
        // The name of the family nothing set is one name, and it is the font `sans-serif` is.
        l.push([named, generic, plain, made].map(t => t.font.family).join(",") + " "
               + [named, generic, made].map(t => t.fontInfo.family === plain.fontInfo.family).join(",") + " "
               + String(Qt.font({family: "Serif"})) + " " + show(made))
        // `font = undefined` of a control: no field is a set one, nothing is written.
        l.push(step("fresh", () => { fresh.font = undefined; return show(fresh) }))
        written.font.pixelSize = 21; written.font.family = "Serif"; written.font.bold = true
        l.push(step("written", () => show(written)))
        l.push(step("written reset", () => { written.font = undefined; return show(written) + written.font.pixelSize + written.font.family + written.font.bold }))
        // `font: undefined` in the document is the same reset.
        l.push(step("document", () => show(none) + show(noneInPane)))
        none.font.pixelSize = 14
        l.push("document " + show(none))
        l.push(step("again", () => { written.font = undefined; return show(written) }))
        written.font.italic = true
        l.push(step("italic", () => show(written)))
        l.push(step("declared", () => { var was = show(declared); declared.font = undefined; return was + show(declared) }))
        l.push(step("whole", () => { var was = show(whole); whole.font = undefined; return was + show(whole) }))
        l.push(step("value", () => { whole.font = Qt.font({family: "Serif", pixelSize: 30}); return show(whole) + whole.font.pixelSize }))
        l.push(step("value reset", () => { whole.font = undefined; return show(whole) + whole.font.pixelSize }))
        l.push(step("bound", () => { var was = show(bound); flag = false; return was + show(bound) + show(side) }))
        l.push(step("bound again", () => { flag = true; return show(bound) + show(side) }))
        l.push(step("side reset", () => { side.font = undefined; flag = false; return show(side) }))
        // The font of a text item has no RESET.
        for (var o of [text, label, field]) {
            o.font.pixelSize = 19
            l.push(step("no reset", () => { o.font = undefined; return show(o) }) + show(o))
        }
        // `undefined` written to a field is the value of the field's type made of it.
        var names = ["family", "styleName", "bold", "weight", "italic", "underline", "overline", "strikeout", "pointSize", "pixelSize", "capitalization", "letterSpacing", "wordSpacing", "hintingPreference", "kerning", "preferShaping", "contextFontMerging", "preferTypoLineMetrics"]
        var values = ["Serif", "Bold", true, 700, true, true, true, true, 14, 21, 1, 2, 3, 1, false, false, true, true]
        for (var i = 0; i < names.length; i++) {
            var k = names[i]
            l.push(step(k, () => {
                fields.font[k] = values[i]
                var was = show(fields) + fields.font[k]
                fields.font[k] = undefined
                var now = show(fields) + fields.font[k]
                fields.font = undefined
                return was + ">" + now
            }))
        }
        // A control and a popup give the fields set in their font to the controls in them.
        var tree = () => [pane, inPane, textInPane, labelInPane, frame, deep, deeper, popup, inPopup, app, inApp, underItem, loose].map(show).join("")
        l.push("tree " + tree())
        inPane.font.pixelSize = 11; deep.font.family = "Sans"; pane.font.underline = true
        l.push("own " + tree())
        inPane.font = undefined; deep.font = undefined
        l.push("own reset " + tree())
        pane.font = undefined; popup.font = undefined; app.font = undefined
        l.push("reset " + tree())
        pane.font.pixelSize = 17; popup.font.pixelSize = 17; app.font.pixelSize = 17
        l.push("set " + tree())
        // The control takes them from where it is moved to, and tells when its font changed.
        l.push(step("moved", () => { loose.parent = pane; return show(loose) }))
        l.push(step("moved", () => { loose.parent = other; return show(loose) }))
        l.push(step("moved", () => { loose.parent = frame; return show(loose) }))
        l.push(step("moved", () => { loose.parent = pane; loose.parent = frame; return show(loose) }))
        l.push(step("moved", () => { loose.parent = root; return show(loose) }))
        l.push(step("moved", () => { loose.parent = inPopup; return show(loose) }))
        l.push(step("given", () => { popup.font.pixelSize = 13; other.font = undefined; return show(loose) }))
        result = l.join(" | ")
    }
}
