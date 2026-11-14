// A delegate over a model of the shell with the one role `modelData` (Quickshell's
// ScriptModel; its ObjectModel has the same role): `modelData`, `model.modelData`, `index`, no
// name of a row object; the model takes no write, so `modelData = v` changes nothing and tells
// nobody; a new `values` adds its rows inside the write. Qt's line is Quickshell 0.3.2's on Qt
// 6.12 (`qs -p`).
// qt: rep str count number:2;rep str modelData string:q;rep str model.modelData string:q;rep str modelData.name undefined;rep str name RefErr(name);rep str model.name undefined;rep str index number:1;rep str model.index number:1;rep str model.model boolean:true;rep str same boolean:true;rep str index= TypeError: Cannot assign to read-only property "index";rep str model.index= TypeError: Cannot assign to read-only property "index";rep str model.extra= number:1;rep str modelData= string:string:q/0;rep str model.modelData= string:string:q/string:q/0;rep obj count number:2;rep obj modelData object;rep obj model.modelData object;rep obj modelData.name string:b;rep obj name RefErr(name);rep obj model.name undefined;rep obj index number:1;rep obj model.index number:1;rep obj model.model boolean:true;rep obj same boolean:true;rep obj index= TypeError: Cannot assign to read-only property "index";rep obj model.index= TypeError: Cannot assign to read-only property "index";rep obj model.extra= number:1;rep obj modelData= string:object/0;rep obj model.modelData= string:object/object/0;rep map count number:2;rep map modelData object;rep map model.modelData object;rep map modelData.name string:mb;rep map name RefErr(name);rep map model.name undefined;rep map index number:1;rep map model.index number:1;rep map model.model boolean:true;rep map same boolean:true;rep map index= TypeError: Cannot assign to read-only property "index";rep map model.index= TypeError: Cannot assign to read-only property "index";rep map model.extra= number:1;rep map modelData= string:object/0;rep map model.modelData= string:object/object/0;req str count number:2;req str modelData string:q;req str modelData.name undefined;req str index number:1;req str model string:undefined;req str modelData= string:W;req obj count number:2;req obj modelData object;req obj modelData.name string:b;req obj index number:1;req obj model string:undefined;req obj modelData= string:W;lv str count number:2;lv str modelData string:q;lv str model.modelData string:q;lv str modelData.name undefined;lv str name RefErr(name);lv str model.name undefined;lv str index number:1;lv str model.index number:1;lv str model.model boolean:true;lv str same boolean:true;lv str index= TypeError: Cannot assign to read-only property "index";lv str model.index= TypeError: Cannot assign to read-only property "index";lv str model.extra= number:1;lv str modelData= string:string:q/0;lv str model.modelData= string:string:q/string:q/0;lv obj count number:2;lv obj modelData object;lv obj model.modelData object;lv obj modelData.name string:b;lv obj name RefErr(name);lv obj model.name undefined;lv obj index number:1;lv obj model.index number:1;lv obj model.model boolean:true;lv obj same boolean:true;lv obj index= TypeError: Cannot assign to read-only property "index";lv obj model.index= TypeError: Cannot assign to read-only property "index";lv obj model.extra= number:1;lv obj modelData= string:object/0;lv obj model.modelData= string:object/object/0;gv map count number:2;gv map modelData object;gv map model.modelData object;gv map modelData.name string:mb;gv map name RefErr(name);gv map model.name undefined;gv map index number:1;gv map model.index number:1;gv map model.model boolean:true;gv map same boolean:true;gv map index= TypeError: Cannot assign to read-only property "index";gv map model.index= TypeError: Cannot assign to read-only property "index";gv map model.extra= number:1;gv map modelData= string:object/0;gv map model.modelData= string:object/object/0;inst count number:2;inst str modelData string:q;inst str model.modelData string:q;inst str index number:1;inst str index= TypeError: Cannot assign to read-only property "index";after strs string:p,q;after smStr string:p,q;after ob.name string:b;after maps string:mb;moved count number:3;moved kept boolean:true;moved row2 string:q/undefined;moved row1 string:z/undefined
import QtQuick
import Rustshell
Item {
    id: root
    width: 200; height: 200
    property string result: ""
    property var seen: []
    function s(v) {
        if (v === undefined) return "undefined"
        if (v === null) return "null"
        if (typeof v === "object" || typeof v === "function") return typeof v
        return typeof v + ":" + v
    }
    function q(fn) { try { return s(fn()) } catch (e) { return ("" + e).replace(/ReferenceError: (\w+) is not defined/, "RefErr($1)") } }
    function t(name, fn) { seen.push(name + " " + q(fn)) }
    QtObject { id: oa; property string name: "a"; property int n: 1 }
    QtObject { id: ob; property string name: "b"; property int n: 2 }
    property var strs: ["p", "q"]
    property var objs: [oa, ob]
    property var maps: [{ name: "ma", n: 1 }, { name: "mb", n: 2 }]
    ScriptModel { id: smStr; values: root.strs }
    ScriptModel { id: smObj; values: root.objs }
    ScriptModel { id: smMap; values: root.maps }
    component D: Item {
        readonly property var cnt: ({ n: 0 })
        property var seenData: { cnt.n++; return modelData }
        function go(tag) {
            root.t(tag + " modelData", () => modelData)
            root.t(tag + " model.modelData", () => model.modelData)
            root.t(tag + " modelData.name", () => modelData.name)
            root.t(tag + " name", () => name)
            root.t(tag + " model.name", () => model.name)
            root.t(tag + " index", () => index)
            root.t(tag + " model.index", () => model.index)
            root.t(tag + " model.model", () => model.model === model)
            root.t(tag + " same", () => modelData === model.modelData)
            root.t(tag + " index=", () => { index = 7; return index })
            root.t(tag + " model.index=", () => { model.index = 7; return model.index })
            root.t(tag + " model.extra=", () => { model.extra = 1; return model.extra })
            var e = cnt.n
            root.t(tag + " modelData=", () => { modelData = "W"; return root.s(modelData) + "/" + (cnt.n - e) })
            e = cnt.n
            root.t(tag + " model.modelData=", () => { model.modelData = "X"; return root.s(model.modelData) + "/" + root.s(modelData) + "/" + (cnt.n - e) })
        }
    }
    component Req: Item {
        required property var modelData
        required property int index
        function go(tag) {
            root.t(tag + " modelData", () => modelData)
            root.t(tag + " modelData.name", () => modelData.name)
            root.t(tag + " index", () => index)
            root.t(tag + " model", () => typeof model)
            root.t(tag + " modelData=", () => { modelData = "W"; return modelData })
        }
    }
    Repeater { id: rStr; model: smStr; D {} }
    Repeater { id: rObj; model: smObj; D {} }
    Repeater { id: rMap; model: smMap; D {} }
    Repeater { id: rqStr; model: smStr; Req {} }
    Repeater { id: rqObj; model: smObj; Req {} }
    ListView { id: lStr; width: 100; height: 100; model: smStr; delegate: D { width: 10; height: 10 } }
    ListView { id: lObj; width: 100; height: 100; model: smObj; delegate: D { width: 10; height: 10 } }
    GridView { id: gMap; width: 100; height: 100; model: smMap; delegate: D { width: 10; height: 10 } }
    Instantiator { id: iStr; model: smStr; delegate: QtObject {
        function go(tag) {
            root.t(tag + " modelData", () => modelData); root.t(tag + " model.modelData", () => model.modelData); root.t(tag + " index", () => index)
            root.t(tag + " index=", () => { index = 7; return index })
        } } }
    function run() {
        var reps = { "rep str": rStr, "rep obj": rObj, "rep map": rMap, "req str": rqStr, "req obj": rqObj }
        for (var k in reps) {
            t(k + " count", () => reps[k].count)
            try { reps[k].itemAt(1).go(k) } catch (e) { seen.push(k + " FAILED " + e) }
        }
        var views = { "lv str": lStr, "lv obj": lObj, "gv map": gMap }
        for (var k in views) {
            t(k + " count", () => views[k].count)
            try { views[k].itemAtIndex(1).go(k) } catch (e) { seen.push(k + " FAILED " + e) }
        }
        t("inst count", () => iStr.count)
        try { iStr.objectAt(1).go("inst str") } catch (e) { seen.push("inst FAILED " + e) }
        // the values after the writes
        t("after strs", () => root.strs.join(",")); t("after smStr", () => smStr.values.join(","))
        t("after ob.name", () => ob.name); t("after maps", () => root.maps[1].name)
        // a new array: the delegates of the rows that stay are kept
        var keep = rStr.itemAt(0)
        root.strs = ["p", "z", "q"]
        t("moved count", () => rStr.count)
        t("moved kept", () => rStr.itemAt(0) === keep)
        t("moved row2", () => rStr.itemAt(2).seenData + "/" + rStr.itemAt(2).index)
        t("moved row1", () => rStr.itemAt(1).seenData + "/" + rStr.itemAt(1).index)
        result = seen.join(";")
    }
    Component.onCompleted: Qt.callLater(run)
}
