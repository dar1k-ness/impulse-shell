// A write to a component of a `color` (`c.r = 0.5`) changes the colour where it was read from,
// as in Qt 6.12: a property (by its bare name, `root.c`), a local, a member of an object. The
// property sends one change signal when the colour is another one and none for an equal
// component; `c.valid` refuses the write and an unknown name is ignored. Each step: the
// signals of c and cv it sent, then its value.
// Not as Qt, and not here: `var t = c; t.r = 1` changes `c` too in Qt; a component out of
// 0..1 (`c.r = 2`) is kept by Qt; a `property var` holding a colour sends no signal for a
// component write in Qt; `rect.color.r = 0.5`.
// qt: a10,[0.5000076293945312,"#800000"];b00,"#800000";c30,"#8080ff40";d10,["#8040ffff",0.5];e10,["#80008080",0.2500038146972656];f30,"#807c6543";g00,"Error:Cannot assign bool to bool";h20,["#8000ff",0.5000076293945312];i10,["#0000ff",0];j00,[null,"#0000ff"];k10,["#4000ff"];l20,["#ff00ff"];m20,"#ff8040";n00,"#80ff00";o00,["#808000",0.5000076293945312,0.5000076293945312];p00,["#00ffff"];q10,[0.062745101749897,0.125490203499794,0.1882352977991104,1,0.5833333134651184,0.6666666865348816,0.1882352977991104,0.5833333134651184,0.5000076293945312,0.125490203499794,true];r00,"#102030";s10,"#804020";t20,"#4d336699";u00,["#808000","#808000"];v00,"#ff0080";w00,"#808000";
import QtQuick
Item {
    id: root
    property string result: ""
    property color c: "red"
    property int nc: 0
    onCChanged: nc++
    property var cv: Qt.rgba(1, 0, 0, 1)
    property int ncv: 0
    onCvChanged: ncv++
    function step(k, f) {
        var n = [nc, ncv]
        var r
        try { r = f() } catch (e) { r = e.name + ":" + e.message }
        var now = [nc, ncv]
        result += k + now.map((c, i) => c - n[i]).join("") + "," + JSON.stringify(r) + ";"
    }
    Component.onCompleted: {
        step("a", () => { c.r = 0.5; return [c.r, "" + c] })
        step("b", () => { c.r = c.r; return "" + c })
        step("c", () => { c.g = 1; c.b = 0.25; c.a = 0.5; return "" + c })
        step("d", () => { c.hsvHue = 0.5; return ["" + c, c.hsvHue] })
        step("e", () => { c.hslLightness = 0.25; return ["" + c, c.hslLightness] })
        step("f", () => { c.hsvSaturation = 0.5; c.hsvValue = 0.5; c.hslHue = 0.1; c.hslSaturation = 0.3; return "" + c })
        step("g", () => { c.valid = false; return [c.valid, "" + c] })
        step("h", () => { c = "blue"; c.r = "0.5"; return ["" + c, c.r] })
        step("i", () => { c.r = undefined; return ["" + c, c.r] })
        step("j", () => { c.zz = 3; return [c.zz, "" + c] })
        step("k", () => { c.r += 0.25; return ["" + c] })
        step("l", () => { c.r = 0; c.r++; return ["" + c] })
        step("m", () => { root.c.g = 0.5; this.c.b = 0.25; return "" + root.c })
        step("n", () => { var o = { k: Qt.rgba(1, 0, 0, 1) }; o.k.r = 0.5; o.k.g = 1; return "" + o.k })
        step("o", () => { var o = Qt.rgba(1, 0, 0, 1); o.r = 0.5; o.g = 0.5; return ["" + o, o.r, o.g] })
        step("p", () => { var o = Qt.rgba(1, 0, 0, 1); o.hsvHue = 0.5; return ["" + o] })
        step("q", () => { c = "#102030"; return [c.r, c.g, c.b, c.a, c.hsvHue, c.hsvSaturation, c.hsvValue, c.hslHue, c.hslSaturation, c.hslLightness, c.valid] })
        step("r", () => { c = "#102030"; c.r = 16 / 255; return "" + c })
        step("s", () => { c = Qt.rgba(0.5, 0.25, 0.125, 1); c.r = 0.5; c.g = 0.25; return "" + c })
        step("t", () => { c = Qt.rgba(0.2, 0.4, 0.6, 1); c.a = 0.3; c.a = 0.3; return "" + c })
        step("u", () => { const k = Qt.rgba(1, 0, 0, 1); k.r = 0.5; const g = () => { k.g = 0.5; return "" + k }; return [g(), "" + k] })
        step("v", () => (function (k) { k.b = 0.5; return "" + k })(Qt.rgba(1, 0, 0, 1)))
        step("w", () => { var o = [Qt.rgba(1, 0, 0, 1)], i = 0; o[0].r = 0.5; o[i].g = 0.5; return "" + o[0] })
    }
}
