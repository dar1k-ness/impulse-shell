// A `date` property and a Date in a `property var`, as in Qt 6.12. A `date` property reads as
// a new Date each time (`d === d` is false); a method setting the time, called on the property
// (`d.setFullYear(2021)`), stores the new time there and sends one change signal when the
// time is another one, and called on a variable holding the read value changes only that.
// Either kind of property keeps a copy of a Date stored in it. A `property var` hands out the
// one Date it keeps, and a method changing it sends no signal.
// Each step: the signals of d and dv it sent, then its value.
// qt: a00,["object",true,"[object Date]",2020];b10,2021;c00,2021;d00,[2022,2021,false];e60,[5,3,4,5,6,7];f10,0;g00,0;h70,"2001-02-02T03:04:05.006Z";i10,[null,"Invalid Date"];j10,2020;k00,0;l10,[2020,2030];m00,[false,false,true];n00,undefined;o00,[1998,2030];p10,1999;q00,["object",true,"[object Date]",2020];r00,2021;s00,2021;t00,[2022,2022,true];u00,0;v01,0;w01,[2022,2030];x01,[2041,2040,false];y10,[2041,2040,false];z00,0;A00,5;B01,[1980,2040,false];C10,[1980];D00,0;E10,0;
import QtQuick
Item {
    id: root
    property string result: ""
    property date d: new Date(2020, 1, 1)
    property int nd: 0
    onDChanged: nd++
    property var dv: new Date(2020, 1, 1)
    property int ndv: 0
    onDvChanged: ndv++
    function step(k, f) {
        var n = [nd, ndv]
        var r
        try { r = f() } catch (e) { r = e.name + ":" + e.message }
        var now = [nd, ndv]
        result += k + now.map((c, i) => c - n[i]).join("") + "," + JSON.stringify(r) + ";"
    }
    Component.onCompleted: {
        step("a", () => [typeof d, d instanceof Date, Object.prototype.toString.call(d), d.getFullYear()])
        step("b", () => { d.setFullYear(2021); return d.getFullYear() })
        step("c", () => { d.setFullYear(2021); return d.getFullYear() })
        step("d", () => { var t = d; t.setFullYear(2022); return [t.getFullYear(), d.getFullYear(), t === d] })
        step("e", () => { d.setMonth(5); d.setDate(3); d.setHours(4); d.setMinutes(5); d.setSeconds(6); d.setMilliseconds(7); return [d.getMonth(), d.getDate(), d.getHours(), d.getMinutes(), d.getSeconds(), d.getMilliseconds()] })
        step("f", () => { d.setTime(0); return d.getTime() })
        step("g", () => { d.setTime(0); return d.getTime() })
        step("h", () => { d.setUTCFullYear(2001); d.setUTCMonth(1); d.setUTCDate(2); d.setUTCHours(3); d.setUTCMinutes(4); d.setUTCSeconds(5); d.setUTCMilliseconds(6); return d.toISOString() })
        step("i", () => { d.setTime(NaN); return [d.getTime(), "" + d] })
        step("j", () => { d = new Date(2020, 1, 1); return d.getFullYear() })
        step("k", () => { d = d; return 0 })
        step("l", () => { var t = d; d = new Date(2030, 0, 1); return [t.getFullYear(), d.getFullYear()] })
        step("m", () => { return [d === d, d == d, d.getTime() === d.getTime()] })
        step("n", () => { d.zz = 5; return d.zz })
        step("o", () => { var t = d; t.setFullYear(1999); t.setFullYear(1998); return [t.getFullYear(), d.getFullYear()] })
        step("p", () => { d.setYear(99); return d.getFullYear() })
        step("q", () => [typeof dv, dv instanceof Date, Object.prototype.toString.call(dv), dv.getFullYear()])
        step("r", () => { dv.setFullYear(2021); return dv.getFullYear() })
        step("s", () => { dv.setFullYear(2021); return dv.getFullYear() })
        step("t", () => { var t = dv; t.setFullYear(2022); return [t.getFullYear(), dv.getFullYear(), t === dv] })
        step("u", () => { dv = dv; return 0 })
        step("v", () => { dv = new Date(dv.getTime()); return 0 })
        step("w", () => { var t = dv; dv = new Date(2030, 0, 1); return [t.getFullYear(), dv.getFullYear()] })
        step("x", () => { var t = new Date(2040, 0, 1); dv = t; t.setFullYear(2041); return [t.getFullYear(), dv.getFullYear(), t === dv] })
        step("y", () => { var t = new Date(2040, 0, 1); d = t; t.setFullYear(2041); return [t.getFullYear(), d.getFullYear(), t === d] })
        step("z", () => { dv.setTime(dv.getTime()); return 0 })
        step("A", () => { dv.zz = 5; return dv.zz })
        step("B", () => { dv = d; dv.setFullYear(1980); return [dv.getFullYear(), d.getFullYear(), dv === d] })
        step("C", () => { d = dv; return [d.getFullYear()] })
        step("D", () => { d.setFullYear(d.getFullYear()); return 0 })
        step("E", () => { d.setHours(d.getHours() + 1); return 0 })
    }
}
