// A delegate over a FolderListModel (Qt 6.12): its roles are names of the context and of
// `model`, `modelData` is nothing (the model item for a delegate that requires it), a write to
// a role is dropped and tells nobody, a new name on `model` is kept. The rows are the files of
// `folder_rows`.
// qt: count number:3;rep count number:3;rep fileName string:b.txt;rep model.fileName string:b.txt;rep fileIsDir boolean:false;rep fileSuffix string:txt;rep fileBaseName string:b;rep fileSize number:2;rep filePath string:rows/b.txt;rep fileUrl string:rows/b.txt;rep fileURL RefErr(fileURL);rep typeof fileModified string:object;rep modelData undefined;rep model.modelData undefined;rep index number:1;rep model.index number:1;rep model.model boolean:true;rep nope RefErr(nope);rep model.nope undefined;rep index= TypeError: Cannot assign to read-only property "index";rep fileName= string:b.txt/0;rep model.fileName= string:b.txt/b.txt/0;rep model.extra= number:1;rep get string:b.txt;req count number:3;req modelData object;req modelData.fileName string:b.txt;req modelData.fileIsDir boolean:false;req modelData.filePath string:rows/b.txt;req modelData.index number:1;req modelData.modelData boolean:true;req index number:1;req model string:undefined;req fileName RefErr(fileName);reqrole count number:3;reqrole fileName string:b.txt;reqrole fileIsDir boolean:false;reqrole index RefErr(index);reqrole fileName= string:W/b.txt;rep dir string:sub/1;lv count number:3;lv fileName string:b.txt;lv model.fileName string:b.txt;lv fileIsDir boolean:false;lv fileSuffix string:txt;lv fileBaseName string:b;lv fileSize number:2;lv filePath string:rows/b.txt;lv fileUrl string:rows/b.txt;lv fileURL RefErr(fileURL);lv typeof fileModified string:object;lv modelData undefined;lv model.modelData undefined;lv index number:1;lv model.index number:1;lv model.model boolean:true;lv nope RefErr(nope);lv model.nope undefined;lv index= TypeError: Cannot assign to read-only property "index";lv fileName= string:b.txt/0;lv model.fileName= string:b.txt/b.txt/0;lv model.extra= number:1;lv get string:b.txt;gv count number:3;gv modelData object;gv modelData.fileName string:b.txt;gv modelData.fileIsDir boolean:false;gv modelData.filePath string:rows/b.txt;gv modelData.index number:1;gv modelData.modelData boolean:true;gv index number:1;gv model string:undefined;gv fileName RefErr(fileName)
import QtQuick
import Qt.labs.folderlistmodel
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
    FolderListModel { id: fm; folder: Qt.resolvedUrl("folder_rows"); showDirs: true; sortField: FolderListModel.Name
        onCountChanged: if (count === 3) Qt.callLater(root.run) }
    component D: Item {
        readonly property var cnt: ({ n: 0 })
        property var seenName: { cnt.n++; return fileName }
        function go(tag) {
            root.t(tag + " fileName", () => fileName)
            root.t(tag + " model.fileName", () => model.fileName)
            root.t(tag + " fileIsDir", () => fileIsDir)
            root.t(tag + " fileSuffix", () => fileSuffix)
            root.t(tag + " fileBaseName", () => fileBaseName)
            root.t(tag + " fileSize", () => fileSize)
            root.t(tag + " filePath", () => filePath.replace(/.*\/folder_rows\//, "rows/"))
            root.t(tag + " fileUrl", () => ("" + fileUrl).replace(/.*\/folder_rows\//, "rows/"))
            root.t(tag + " fileURL", () => ("" + fileURL).replace(/.*\/folder_rows\//, "rows/"))
            root.t(tag + " typeof fileModified", () => typeof fileModified)
            root.t(tag + " modelData", () => modelData)
            root.t(tag + " model.modelData", () => model.modelData)
            root.t(tag + " index", () => index)
            root.t(tag + " model.index", () => model.index)
            root.t(tag + " model.model", () => model.model === model)
            root.t(tag + " nope", () => nope)
            root.t(tag + " model.nope", () => model.nope)
            root.t(tag + " index=", () => { index = 7; return index })
            var e = cnt.n
            root.t(tag + " fileName=", () => { fileName = "W"; return fileName + "/" + (cnt.n - e) })
            e = cnt.n
            root.t(tag + " model.fileName=", () => { model.fileName = "X"; return model.fileName + "/" + fileName + "/" + (cnt.n - e) })
            root.t(tag + " model.extra=", () => { model.extra = 1; return model.extra })
            root.t(tag + " get", () => fm.get(1, "fileName"))
        }
    }
    component Req: Item {
        required property var modelData
        required property int index
        function go(tag) {
            root.t(tag + " modelData", () => modelData)
            root.t(tag + " modelData.fileName", () => modelData.fileName)
            root.t(tag + " modelData.fileIsDir", () => modelData.fileIsDir)
            root.t(tag + " modelData.filePath", () => modelData.filePath.replace(/.*\/folder_rows\//, "rows/"))
            root.t(tag + " modelData.index", () => modelData.index)
            root.t(tag + " modelData.modelData", () => modelData.modelData === modelData)
            root.t(tag + " index", () => index)
            root.t(tag + " model", () => typeof model)
            root.t(tag + " fileName", () => fileName)
        }
    }
    component ReqRole: Item {
        required property string fileName
        required property bool fileIsDir
        function go(tag) {
            root.t(tag + " fileName", () => fileName)
            root.t(tag + " fileIsDir", () => fileIsDir)
            root.t(tag + " index", () => index)
            root.t(tag + " fileName=", () => { fileName = "W"; return fileName + "/" + fm.get(1, "fileName") })
        }
    }
    Repeater { id: rep; model: fm; D {} }
    Repeater { id: rq; model: fm; Req {} }
    Repeater { id: rr; model: fm; ReqRole {} }
    ListView { id: lv; width: 100; height: 100; model: fm; delegate: D { width: 10; height: 10 } }
    GridView { id: gv; width: 100; height: 100; model: fm; delegate: Req { width: 10; height: 10 } }
    function run() {
        t("count", () => fm.count)
        var reps = { "rep": rep, "req": rq, "reqrole": rr }
        for (var k in reps) {
            t(k + " count", () => reps[k].count)
            try { reps[k].itemAt(1).go(k) } catch (e) { seen.push(k + " FAILED " + e) }
        }
        t("rep dir", () => rep.itemAt(2).seenName + "/" + rep.itemAt(2).cnt.n)
        lv.forceLayout(); gv.forceLayout()
        t("lv count", () => lv.count)
        try { lv.itemAtIndex(1).go("lv") } catch (e) { seen.push("lv FAILED " + e) }
        t("gv count", () => gv.count)
        try { gv.itemAtIndex(1).go("gv") } catch (e) { seen.push("gv FAILED " + e) }
        result = seen.join(";")
    }
}
