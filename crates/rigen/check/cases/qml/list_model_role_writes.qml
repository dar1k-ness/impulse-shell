// A write of a value of another type to a ListModel role (Qt 6.12): through `model.role`, the
// role's name, `get(i).role` and setProperty the value is converted to the role's type, as a
// QVariant is.
// qt: str=5 string:string:5/string:5;str=true string:string:true/string:true;str=null string:string:true/string:true;str=undefined string:string:true/string:true;str={} string:string:/string:;num='7' string:number:7/number:7;num='x' string:number:0/number:0;num=true string:number:1/number:1;num=null string:number:1/number:1;num=undefined string:number:1/number:1;flag=1 string:boolean:true/boolean:true;flag='x' string:boolean:true/boolean:true;flag='' string:boolean:false/boolean:false;flag=0 string:boolean:false/boolean:false;lm names string:number:77,number:78,boolean:false,number,number,boolean;lm model= TypeError: Cannot assign to read-only property "model";lm row= string:5/5;num names string:number:5,number:78,string:outer,number,number,undefined;num model= TypeError: Cannot assign to read-only property "model";num row= string:5/5;sp str=5 string:5;sp num='7' number:7;sp flag=1 boolean:true;get str=5 string:5;get num='7' number:7
import QtQuick
import QtQml.Models
Item {
    id: root
    property var seen: []
    property string result: seen.join(";")
    property int row: 77
    property int column: 78
    property string hasModelChildren: "outer"
    function s(v) { return typeof v + ":" + v }
    function t(tag, fn) { try { seen.push(tag + " " + s(fn())) } catch (e) { seen.push(tag + " " + e) } }
    ListModel { id: lm; ListElement { str: "a"; num: 1; flag: true } ListElement { str: "b"; num: 2; flag: false } }
    Repeater { id: rep; model: lm; delegate: Item {
        function w(k, v) { model[k] = v; return typeof lm.get(index)[k] + ":" + lm.get(index)[k] + "/" + typeof model[k] + ":" + model[k] }
        function names() { return [typeof row + ":" + row, typeof column + ":" + column, typeof hasModelChildren + ":" + hasModelChildren, typeof model.row, typeof model.column, typeof model.hasModelChildren].join(",") }
        function setModel() { model = 5; return typeof model }
        function setRow() { row = 5; return row + "/" + root.row }
    } }
    Repeater { id: repn; model: 2; delegate: Item {
        function names() { return [typeof row + ":" + row, typeof column + ":" + column, typeof hasModelChildren + ":" + hasModelChildren, typeof model.row, typeof model.column, typeof model.hasModelChildren].join(",") }
        function setModel() { model = 5; return typeof model }
        function setRow() { row = 5; return row + "/" + root.row }
    } }
    Component.onCompleted: {
        var d = rep.itemAt(0)
        t("str=5", () => d.w("str", 5))
        t("str=true", () => d.w("str", true))
        t("str=null", () => d.w("str", null))
        t("str=undefined", () => d.w("str", undefined))
        t("str={}", () => d.w("str", ({})))
        t("num='7'", () => d.w("num", "7"))
        t("num='x'", () => d.w("num", "x"))
        t("num=true", () => d.w("num", true))
        t("num=null", () => d.w("num", null))
        t("num=undefined", () => d.w("num", undefined))
        t("flag=1", () => d.w("flag", 1))
        t("flag='x'", () => d.w("flag", "x"))
        t("flag=''", () => d.w("flag", ""))
        t("flag=0", () => d.w("flag", 0))
        t("lm names", () => d.names())
        t("lm model=", () => d.setModel())
        t("lm row=", () => d.setRow())
        var e = repn.itemAt(1)
        t("num names", () => e.names())
        t("num model=", () => e.setModel())
        t("num row=", () => e.setRow())
        t("sp str=5", () => { lm.setProperty(1, "str", 5); return lm.get(1).str })
        t("sp num='7'", () => { lm.setProperty(1, "num", "7"); return lm.get(1).num })
        t("sp flag=1", () => { lm.setProperty(1, "flag", 1); return lm.get(1).flag })
        t("get str=5", () => { lm.get(1).str = 5; return lm.get(1).str })
        t("get num='7'", () => { lm.get(1).num = "7"; return lm.get(1).num })
        seenChanged()
    }
}
