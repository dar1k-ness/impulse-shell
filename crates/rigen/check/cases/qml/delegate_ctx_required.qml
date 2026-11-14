// What hides a role in a delegate and what `required property` does (Qt 6.12): a delegate with
// a required property has none of the context's names, its required properties are set from
// the row and follow it until a script writes them; one that no role fills keeps the delegate
// from being made.
// errors: 1 (Qt warns once too: Required property nope was not initialized)
// qt: count number:2;own name string:own;own model.name string:b;own d.name string:own;own child string:own/kidn/2;own outerOnly string:outer;own write string:w/b/b;count number:2;outer name string:rootname;outer write string:ow/ow;count number:2;odd width number:0;odd model.width undefined;odd x number:0;odd objectName string:;odd visible boolean:true;odd color string:#ffffff;odd model.color undefined;odd text string:u;odd index number:19;odd model.index undefined;odd typeof model string:number;odd model.model undefined;odd modelData string:md2;odd model.modelData undefined;odd parent string:object;odd model.parent undefined;odd outerOnly string:role2;odd child string:number:0/object/string:u/boolean:true/number:19;count number:2;id name string:object;id n number:2;count number:2;req name string:b;req n RefErr(n);req index RefErr(index);req typeof model string:undefined;req modelData RefErr(modelData);req write string:w/b/3;req model set string:w/s/3;req d.name string:w;count number:2;req2 name string:ow;req2 index number:1;req2 model.name string:b;req2 model.index number:1;req2 modelData object;req2 model.modelData object;req2 model write string:w/w/3;req2 index write string:7/1;req2 model set string:s/4;count number:2;req3 string:b/2/1;req3 after set string:s/9;req3 after insert string:s/9/2/3;count number:1;FAILED TypeError: Cannot call method 'go' of null;count number:2;reqArro string:b/2;reqArro index RefErr(index);reqArro modelData string:undefined;reqArro write string:w/b;count number:2;reqArroMd string:b/1;reqArroMd name string:ow;reqArroMd model string:undefined;count number:2;reqObjs string:b;reqObjs n RefErr(n);reqObjs write string:w/b;reqObjs src write string:w;count number:2;reqNum string:1/1;reqNum model string:undefined;lvReq string:b/1;lvReq n RefErr(n);lvReq model string:undefined;access string:number:0
import QtQuick
import QtQml.Models
Item {
    id: root
    width: 400; height: 400
    property string result: ""
    property var seen: []
    property string name: "rootname"
    property string outerOnly: "outer"
    function s(v) {
        if (v === undefined) return "undefined"
        if (v === null) return "null"
        if (typeof v === "object" || typeof v === "function") return typeof v
        return typeof v + ":" + v
    }
    function q(fn) { try { return s(fn()) } catch (e) { return ("" + e).replace(/ReferenceError: (\w+) is not defined/, "RefErr($1)").replace(/TypeError: Cannot read property '(\w+)' of undefined/, "NoProp($1)") } }
    function t(name, fn) { seen.push(name + " " + q(fn)) }
    ListModel { id: lm; ListElement { name: "a"; n: 1 } ListElement { name: "b"; n: 2 } }
    ListModel { id: lmOdd; ListElement { width: 7; x: 3; objectName: "o"; visible: false; color: "red"; text: "t"; index: 9; model: 5; modelData: "md"; parent: 4; outerOnly: "role" } ListElement { width: 8; x: 4; objectName: "p"; visible: false; color: "blue"; text: "u"; index: 19; model: 15; modelData: "md2"; parent: 14; outerOnly: "role2" } }
    ListModel { id: lmReq; ListElement { name: "a"; n: 1 } ListElement { name: "b"; n: 2 } }
    ListModel { id: lmReq2; ListElement { name: "a"; n: 1 } ListElement { name: "b"; n: 2 } }
    ListModel { id: lmReq3; ListElement { name: "a"; n: 1 } ListElement { name: "b"; n: 2 } }
    ListModel { id: lmMiss; ListElement { name: "a" } }
    property var arro: [{ name: "a", n: 1 }, { name: "b", n: 2 }]
    property list<QtObject> objs: [ QtObject { property string name: "a"; property int n: 1 }, QtObject { property string name: "b"; property int n: 2 } ]

    // the delegate's own property hides the role; the outer document's property is hidden by the role
    Repeater { id: own; model: lm; Item {
        id: d1
        property string name: "own"
        function go() {
            root.t("own name", () => name); root.t("own model.name", () => model.name); root.t("own d.name", () => d1.name)
            root.t("own child", () => kid1.f()); root.t("own outerOnly", () => outerOnly)
            root.t("own write", () => { name = "w"; return name + "/" + model.name + "/" + lm.get(1).name })
        }
        Item { id: kid1; property string n: "kidn"; function f() { return name + "/" + n + "/" + model.n } }
    } }
    // no role: the outer document's property
    Repeater { id: outerRep; model: 2; Item { function go() { root.t("outer name", () => name); root.t("outer write", () => { name = "ow"; return name + "/" + root.name }) } } }
    // roles named like members of the delegate, and like the context's own names
    Repeater { id: odd; model: lmOdd; Rectangle {
        id: d2
        function go() {
            root.t("odd width", () => width); root.t("odd model.width", () => model.width); root.t("odd x", () => x); root.t("odd objectName", () => objectName)
            root.t("odd visible", () => visible); root.t("odd color", () => "" + color); root.t("odd model.color", () => model.color)
            root.t("odd text", () => text); root.t("odd index", () => index); root.t("odd model.index", () => model.index)
            root.t("odd typeof model", () => typeof model); root.t("odd model.model", () => model.model); root.t("odd modelData", () => modelData); root.t("odd model.modelData", () => model.modelData)
            root.t("odd parent", () => typeof parent); root.t("odd model.parent", () => model.parent); root.t("odd outerOnly", () => outerOnly)
            root.t("odd child", () => kid2.f())
        }
        Item { id: kid2; function f() { return [root.q(() => width), root.q(() => color), root.q(() => text), root.q(() => visible), root.q(() => index)].join("/") } }
    } }
    // an id of the delegate named like a role
    Repeater { id: idRep; model: lm; Item { function go() { root.t("id name", () => typeof name); root.t("id n", () => n) }  Item { id: name } } }
    // required properties
    Repeater { id: req; model: lmReq; Item {
        id: d3
        required property string name
        property var bName: { root.bump("req"); return name }
        function go() {
            root.t("req name", () => name); root.t("req n", () => n); root.t("req index", () => index); root.t("req typeof model", () => typeof model); root.t("req modelData", () => modelData)
            root.t("req write", () => { name = "w"; return name + "/" + lmReq.get(1).name + "/" + root.cnt.req })
            root.t("req model set", () => { lmReq.setProperty(1, "name", "s"); return name + "/" + lmReq.get(1).name + "/" + root.cnt.req })
            root.t("req d.name", () => d3.name)
        }
    } }
    Repeater { id: req2; model: lmReq2; Item {
        required property int index
        required property var model
        required property var modelData
        property var bName: { root.bump("req2"); return model.name }
        function go() {
            root.t("req2 name", () => name); root.t("req2 index", () => index); root.t("req2 model.name", () => model.name); root.t("req2 model.index", () => model.index); root.t("req2 modelData", () => modelData)
            root.t("req2 model.modelData", () => model.modelData)
            root.t("req2 model write", () => { model.name = "w"; return model.name + "/" + lmReq2.get(1).name + "/" + root.cnt.req2 })
            root.t("req2 index write", () => { index = 7; return index + "/" + model.index })
            root.t("req2 model set", () => { lmReq2.setProperty(1, "name", "s"); return model.name + "/" + root.cnt.req2 })
        }
    } }
    Repeater { id: req3; model: lmReq3; Item {
        required property string name
        required property int n
        required property int index
        function go() {
            root.t("req3", () => name + "/" + n + "/" + index)
            root.t("req3 after set", () => { lmReq3.set(1, { name: "s", n: 9 }); return name + "/" + n })
            root.t("req3 after insert", () => { lmReq3.insert(0, { name: "new", n: 0 }); return name + "/" + n + "/" + index + "/" + req3.count })
        }
    } }
    Repeater { id: reqMiss; model: lmMiss; Item { required property string nope; function go() { root.t("reqMiss", () => nope) } } }
    Repeater { id: reqArro; model: root.arro; Item {
        required property string name
        required property int n
        function go() { root.t("reqArro", () => name + "/" + n); root.t("reqArro index", () => index); root.t("reqArro modelData", () => typeof modelData)
            root.t("reqArro write", () => { name = "w"; return name + "/" + root.arro[1].name }) }
    } }
    Repeater { id: reqArroMd; model: root.arro; Item {
        required property var modelData
        required property int index
        function go() { root.t("reqArroMd", () => modelData.name + "/" + index); root.t("reqArroMd name", () => name); root.t("reqArroMd model", () => typeof model) }
    } }
    Repeater { id: reqObjs; model: root.objs; Item {
        required property string name
        function go() { root.t("reqObjs", () => name); root.t("reqObjs n", () => n)
            root.t("reqObjs write", () => { name = "w"; return name + "/" + root.objs[1].name })
            root.t("reqObjs src write", () => { root.objs[1].name = "sw"; return name }) }
    } }
    Repeater { id: reqNum; model: 2; Item {
        required property int index
        required property var modelData
        function go() { root.t("reqNum", () => index + "/" + modelData); root.t("reqNum model", () => typeof model) }
    } }
    ListView { id: lvReq; width: 100; height: 100; model: lm; delegate: Item {
        width: 10; height: 10
        required property string name
        required property int index
        function go() { root.t("lvReq", () => name + "/" + index); root.t("lvReq n", () => n); root.t("lvReq model", () => typeof model) }
    } }
    property var cnt: ({})
    function bump(k) { cnt[k] = (cnt[k] || 0) + 1 }
    function run() {
        for (var r of [own, outerRep, odd, idRep, req, req2, req3, reqMiss, reqArro, reqArroMd, reqObjs, reqNum]) {
            t("count", () => r.count)
            try { r.itemAt(1).go() } catch (e) { seen.push("FAILED " + e) }
        }
        try { lvReq.itemAtIndex(1).go() } catch (e) { seen.push("FAILED " + e) }
        t("access", () => typeof own.delegateModelAccess + ":" + own.delegateModelAccess)
        result = seen.join(";")
    }
    Component.onCompleted: Qt.callLater(run)
}
