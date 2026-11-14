// A write to a `font` property sends a change signal only when the font is another one, as in
// Qt 6.12: not for an equal member (`f.pixelSize = f.pixelSize`, `f.bold = false` on a font
// that is not bold), not for `f = f` or an equal font. The first `f.letterSpacing = 0` is a
// change (the spacing becomes a set one), and `f.pointSize` on a font sized in pixels is not.
// Each step: the signals of f and fv it sent, then its value.
// qt: a10,20;b00,20;c00,20;d00,false;e10,true;f00,700;g00,"string";h10,"Monospace";i00,"Monospace";j00,0;k00,["Monospace",20,true];l00,0;m10,0;n00,0;o00,700;p10,[400,false];q00,[15,20];r00,[15,20];s10,[12,16];t10,0;u10,0;v00,0;w10,1;x00,0;y00,0;z10,"Bold";A10,[33,33];B00,[33,33];C00,10;D00,11;E10,34;F00,34;
import QtQuick
Item {
    id: root
    property string result: ""
    property font f
    property int nf: 0
    onFChanged: nf++
    property var fv: Qt.font({ pixelSize: 10 })
    property int nfv: 0
    onFvChanged: nfv++
    function step(k, f) {
        var n = [nf, nfv]
        var r
        try { r = f() } catch (e) { r = e.name + ":" + e.message }
        var now = [nf, nfv]
        result += k + now.map((c, i) => c - n[i]).join("") + "," + JSON.stringify(r) + ";"
    }
    Component.onCompleted: {
        step("a", () => { f.pixelSize = 20; return f.pixelSize })
        step("b", () => { f.pixelSize = 20; return f.pixelSize })
        step("c", () => { f.pixelSize = f.pixelSize; return f.pixelSize })
        step("d", () => { f.bold = false; return f.bold })
        step("e", () => { f.bold = true; return f.bold })
        step("f", () => { f.bold = true; return f.weight })
        step("g", () => { f.family = f.family; return typeof f.family })
        step("h", () => { f.family = "Monospace"; return f.family })
        step("i", () => { f.family = "Monospace"; return f.family })
        step("j", () => { f = f; return 0 })
        step("k", () => { f = Qt.font({ family: "Monospace", pixelSize: 20, bold: true }); return [f.family, f.pixelSize, f.bold] })
        step("l", () => { f.italic = false; f.underline = false; f.strikeout = false; f.overline = false; return 0 })
        step("m", () => { f.italic = true; return 0 })
        step("n", () => { f.italic = true; return 0 })
        step("o", () => { f.weight = 700; return f.weight })
        step("p", () => { f.weight = 400; return [f.weight, f.bold] })
        step("q", () => { f.pointSize = 12; return [f.pointSize, f.pixelSize] })
        step("r", () => { f.pointSize = 12; return [f.pointSize, f.pixelSize] })
        step("s", () => { f.pixelSize = 16; return [f.pointSize, f.pixelSize] })
        step("t", () => { f.letterSpacing = 0; return f.letterSpacing })
        step("u", () => { f.letterSpacing = 1; f.wordSpacing = 0; return 0 })
        step("v", () => { f.capitalization = Font.MixedCase; return f.capitalization })
        step("w", () => { f.capitalization = Font.AllUppercase; return f.capitalization })
        step("x", () => { f.kerning = true; f.preferShaping = true; return 0 })
        step("y", () => { f.hintingPreference = Font.PreferDefaultHinting; f.styleName = ""; return 0 })
        step("z", () => { f.styleName = "Bold"; return f.styleName })
        step("A", () => { var t = f; t.pixelSize = 33; return [t.pixelSize, f.pixelSize] })
        step("B", () => { var t = f; t.pixelSize = 33; return [t.pixelSize, f.pixelSize] })
        step("C", () => { fv.pixelSize = 10; return fv.pixelSize })
        step("D", () => { fv.pixelSize = 11; return fv.pixelSize })
        step("E", () => { f.pixelSize++; return f.pixelSize })
        step("F", () => { f.pixelSize += 0; return f.pixelSize })
    }
}
