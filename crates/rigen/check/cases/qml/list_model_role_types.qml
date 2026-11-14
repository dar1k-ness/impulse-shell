// A ListModel role has the type of its first value (Qt 6.12, ListLayout::Role): set, append
// and insert leave out a value of another type, setProperty converts it; the roles of a row
// are in the order the model first saw them.
// qt: set str=5 string:a,number:1,boolean:true;set num='7' string:a,number:1,boolean:true;set flag=1 string:a,number:1,boolean:true;set num=true string:a,number:1,boolean:true;set str=null string:,number:1,boolean:true;set str=undefined string:,number:1,boolean:true;append mismatch string:,number:0,boolean:false;insert mismatch string:,number:9,boolean:false;sp 1.5 string:1.5,number:1,boolean:true;sp num ' 8 ' string:1.5,number:8,boolean:true;sp num '0x10' string:1.5,number:0,boolean:true;sp num '1e2' string:1.5,number:100,boolean:true;sp flag 'false' string:1.5,number:100,boolean:false;sp flag '0' string:1.5,number:100,boolean:false;sp flag 'FALSE' string:1.5,number:100,boolean:false;sp str NaN string:nan,number:100,boolean:false;sp str 1e21 string:1e+21,number:100,boolean:false;sp str -0 string:0,number:100,boolean:false;sp str 0.1 string:0.1,number:100,boolean:false;sp new role number:3,number:0;sp new null undefined:undefined;late first number:0,string:,a,b;late keys order a,b,c|boolean:false
import QtQuick
Item {
    id: root
    property var seen: []
    property string result: seen.join(";")
    function s(v) { return typeof v + ":" + v }
    function row(i) { var r = lm.get(i); return s(r.str) + "," + s(r.num) + "," + s(r.flag) }
    function t(tag, fn) { try { seen.push(tag + " " + fn()) } catch (e) { seen.push(tag + " " + e) } }
    ListModel { id: lm; ListElement { str: "a"; num: 1; flag: true } ListElement { str: "b"; num: 2; flag: false } }
    ListModel { id: late }
    Component.onCompleted: {
        t("set str=5", () => { lm.set(0, { str: 5 }); return row(0) })
        t("set num='7'", () => { lm.set(0, { num: "7" }); return row(0) })
        t("set flag=1", () => { lm.set(0, { flag: 1 }); return row(0) })
        t("set num=true", () => { lm.set(0, { num: true }); return row(0) })
        t("set str=null", () => { lm.set(0, { str: null }); return row(0) })
        t("set str=undefined", () => { lm.set(0, { str: undefined }); return row(0) })
        t("append mismatch", () => { lm.append({ str: 5, num: "7", flag: 1 }); return row(2) })
        t("insert mismatch", () => { lm.insert(0, { str: true, num: 9, flag: "x" }); return row(0) })
        t("sp 1.5", () => { lm.setProperty(1, "str", 1.5); return row(1) })
        t("sp num ' 8 '", () => { lm.setProperty(1, "num", " 8 "); return row(1) })
        t("sp num '0x10'", () => { lm.setProperty(1, "num", "0x10"); return row(1) })
        t("sp num '1e2'", () => { lm.setProperty(1, "num", "1e2"); return row(1) })
        t("sp flag 'false'", () => { lm.setProperty(1, "flag", "false"); return row(1) })
        t("sp flag '0'", () => { lm.setProperty(1, "flag", "0"); return row(1) })
        t("sp flag 'FALSE'", () => { lm.setProperty(1, "flag", "FALSE"); return row(1) })
        t("sp str NaN", () => { lm.setProperty(1, "str", NaN); return row(1) })
        t("sp str 1e21", () => { lm.setProperty(1, "str", 1e21); return row(1) })
        t("sp str -0", () => { lm.setProperty(1, "str", -0); return row(1) })
        t("sp str 0.1", () => { lm.setProperty(1, "str", 0.1); return row(1) })
        t("sp new role", () => { lm.setProperty(1, "extra", 3); return s(lm.get(1).extra) + "," + s(lm.get(0).extra) })
        t("sp new null", () => { lm.setProperty(1, "nul", null); return s(lm.get(1).nul) })
        t("late first", () => { late.append({ a: 1 }); late.append({ a: "x", b: "s" }); return s(late.get(1).a) + "," + s(late.get(0).b) + "," + Object.keys(late.get(0)) })
        t("late keys order", () => { late.append({ c: true, b: "t", a: 3 }); return Object.keys(late.get(2)) + "|" + s(late.get(0).c) })
        seenChanged()
    }
}
