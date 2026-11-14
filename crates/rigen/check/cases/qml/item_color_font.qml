// A write to a component of the colour of a Qt type (`rect.color.r = 0.5`, a Text, a TextInput,
// a GradientStop, `border.color`) changes that colour, as in Qt 6.12: read by script it is a
// `color` whatever it was written as, one change signal for another colour and none for the
// same one. A write to a component of the `font` of a Text or TextInput (`tx.font.bold = true`)
// sends `fontChanged` once when the font is another one: `bold` and the weight are one value,
// the pixel size gives the point size, a point size after a pixel size is refused.
// Each step: the signals it sent (colour of rect, fonts, colours of the texts), then its value.
// Not as Qt, and not here: `border.colorChanged` (Qt sends it for the same colour too);
// `var c = rect.color; c.r = 0` changes `rect.color` in Qt; `rect.color.r = 2` is kept by Qt;
// `tx.font = tx.font` (nothing in Qt, an error here).
// qt: a100,[0.5000076293945312,"#800000"];b000,"#800000";c200,"#4080ff00";d100,"#4000ffff";e100,"#40008080";f000,"#0000ff";h001,"#ff2030";i001,"#00ff00";j000,"#ffff00";k000,"Error:Cannot assign bool to bool";l000,[null,"#40008080"];n100,"#40408080";p010,[true,700];q000,true;r010,[20,15];s000,20;t010,0;u000,0;w010,[30,30];x000,[700,true];y010,[400,false];z020,[true,true];A000,[30,22.5];B000,[30,22.5];C010,1;D010,true;
import QtQuick
Item {
    id: root
    property string result: ""
    property int nc: 0
    property int nf: 0
    property int ntc: 0
    Rectangle { id: rect; color: "#ff0000"; border.color: "#000000"; onColorChanged: root.nc++
        gradient: Gradient { GradientStop { id: gs; position: 0; color: "#00ff00" } } }
    Text { id: tx; text: "a"; color: "#102030"; onFontChanged: root.nf++; onColorChanged: root.ntc++
    }
    TextInput { id: ti; text: "a"; onFontChanged: root.nf++; onColorChanged: root.ntc++ }
    function step(k, f) {
        var n = [nc, nf, ntc]
        var r
        try { r = f() } catch (e) { r = e.name + ":" + e.message }
        var now = [nc, nf, ntc]
        result += k + now.map((c, i) => c - n[i]).join("") + "," + JSON.stringify(r) + ";"
    }
    Component.onCompleted: {
        step("a", () => { rect.color.r = 0.5; return [rect.color.r, "" + rect.color] })
        step("b", () => { rect.color.r = 0.5; return "" + rect.color })
        step("c", () => { rect.color.a = 0.25; rect.color.g = 1; return "" + rect.color })
        step("d", () => { rect.color.hsvHue = 0.5; return "" + rect.color })
        step("e", () => { rect.color.hslLightness = 0.25; return "" + rect.color })
        step("f", () => { rect.border.color.b = 1; return "" + rect.border.color })
        step("h", () => { tx.color.r = 1; return "" + tx.color })
        step("i", () => { ti.color.g = 1; return "" + ti.color })
        step("j", () => { gs.color.r = 1; return "" + gs.color })
        step("k", () => { rect.color.valid = false; return "" + rect.color })
        step("l", () => { rect.color.nope = 1; return [rect.color.nope, "" + rect.color] })
        step("n", () => { rect.color.r += 0.25; return "" + rect.color })
        step("p", () => { tx.font.bold = true; return [tx.font.bold, tx.font.weight] })
        step("q", () => { tx.font.bold = true; return tx.font.bold })
        step("r", () => { tx.font.pixelSize = 20; return [tx.font.pixelSize, tx.font.pointSize] })
        step("s", () => { tx.font.pixelSize = 20; tx.font.italic = false; tx.font.family = tx.font.family; return tx.font.pixelSize })
        step("t", () => { tx.font.letterSpacing = 0; return tx.font.letterSpacing })
        step("u", () => { tx.font.letterSpacing = 0; return tx.font.letterSpacing })
        step("w", () => { var f = tx.font; f.pixelSize = 30; return [f.pixelSize, tx.font.pixelSize] })
        step("x", () => { tx.font.weight = 700; return [tx.font.weight, tx.font.bold] })
        step("y", () => { tx.font.weight = 400; return [tx.font.weight, tx.font.bold] })
        step("z", () => { ti.font.bold = true; ti.font.bold = true; ti.font.underline = true; return [ti.font.bold, ti.font.underline] })
        step("A", () => { tx.font.pointSize = 12; return [tx.font.pixelSize, tx.font.pointSize] })
        step("B", () => { tx.font.pointSize = 12; return [tx.font.pixelSize, tx.font.pointSize] })
        step("C", () => { tx.font.capitalization = Font.AllUppercase; tx.font.capitalization = Font.AllUppercase; return tx.font.capitalization })
        step("D", () => { tx.font.bold = "yes"; return tx.font.bold })
    }
}
