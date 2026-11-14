// The names a delegate's context has (Qt 6.12, QQmlDelegateModelItem): the roles, `model`,
// `modelData` and `index`, for a ListModel, a ListModel of one role, a number, an array of
// values, an array of JS objects and a list of objects, in a Repeater, a ListView, a GridView
// and an Instantiator; and which of them a property of the delegate, an id and a name of the
// document hide.
// qt: rep lm count number:3;rep lm name string:b;rep lm n number:2;rep lm modelData undefined;rep lm modelData.name TypeError: Cannot read property 'name' of undefined;rep lm index number:1;rep lm typeof model string:object;rep lm model.name string:b;rep lm model.n number:2;rep lm model.modelData undefined;rep lm model.modelData.name TypeError: Cannot read property 'name' of undefined;rep lm model.index number:1;rep lm model.nope undefined;rep lm outer string:outerprop;rep lm child string:b/number:1/undefined/string:object;rep lm outside undefined/undefined/undefined/string:undefined/boolean:false/boolean:false;lv lm count number:3;lv lm name string:b;lv lm n number:2;lv lm modelData undefined;lv lm modelData.name TypeError: Cannot read property 'name' of undefined;lv lm index number:1;lv lm typeof model string:object;lv lm model.name string:b;lv lm model.n number:2;lv lm model.modelData undefined;lv lm model.modelData.name TypeError: Cannot read property 'name' of undefined;lv lm model.index number:1;lv lm model.nope undefined;lv lm outer string:outerprop;lv lm child string:b/number:1/undefined/string:object;lv lm outside undefined/undefined/undefined/string:undefined/boolean:false/boolean:false;gv lm count number:3;gv lm name string:b;gv lm n number:2;gv lm modelData undefined;gv lm modelData.name TypeError: Cannot read property 'name' of undefined;gv lm index number:1;gv lm typeof model string:object;gv lm model.name string:b;gv lm model.n number:2;gv lm model.modelData undefined;gv lm model.modelData.name TypeError: Cannot read property 'name' of undefined;gv lm model.index number:1;gv lm model.nope undefined;gv lm outer string:outerprop;gv lm child string:b/number:1/undefined/string:object;gv lm outside undefined/undefined/undefined/string:undefined/boolean:false/boolean:false;inst lm count number:3;inst lm name string:b;inst lm n number:2;inst lm modelData undefined;inst lm modelData.name TypeError: Cannot read property 'name' of undefined;inst lm index number:1;inst lm typeof model string:object;inst lm model.name string:b;inst lm model.n number:2;inst lm model.modelData undefined;inst lm model.modelData.name TypeError: Cannot read property 'name' of undefined;inst lm model.index number:1;inst lm model.nope undefined;inst lm outer string:outerprop;inst lm child string:b/number:1/undefined/string:object;inst lm outside undefined/undefined/undefined/string:undefined/boolean:false/boolean:false;rep lm1 count number:3;rep lm1 name string:b;rep lm1 n RefErr(n);rep lm1 modelData string:b;rep lm1 modelData.name undefined;rep lm1 index number:1;rep lm1 typeof model string:object;rep lm1 model.name string:b;rep lm1 model.n undefined;rep lm1 model.modelData string:b;rep lm1 model.modelData.name undefined;rep lm1 model.index number:1;rep lm1 model.nope undefined;rep lm1 outer string:outerprop;rep lm1 child string:b/number:1/string:b/string:object;rep lm1 outside undefined/undefined/undefined/string:undefined/boolean:false/boolean:false;lv lm1 count number:3;lv lm1 name string:b;lv lm1 n RefErr(n);lv lm1 modelData string:b;lv lm1 modelData.name undefined;lv lm1 index number:1;lv lm1 typeof model string:object;lv lm1 model.name string:b;lv lm1 model.n undefined;lv lm1 model.modelData string:b;lv lm1 model.modelData.name undefined;lv lm1 model.index number:1;lv lm1 model.nope undefined;lv lm1 outer string:outerprop;lv lm1 child string:b/number:1/string:b/string:object;lv lm1 outside undefined/undefined/undefined/string:undefined/boolean:false/boolean:false;gv lm1 count number:3;gv lm1 name string:b;gv lm1 n RefErr(n);gv lm1 modelData string:b;gv lm1 modelData.name undefined;gv lm1 index number:1;gv lm1 typeof model string:object;gv lm1 model.name string:b;gv lm1 model.n undefined;gv lm1 model.modelData string:b;gv lm1 model.modelData.name undefined;gv lm1 model.index number:1;gv lm1 model.nope undefined;gv lm1 outer string:outerprop;gv lm1 child string:b/number:1/string:b/string:object;gv lm1 outside undefined/undefined/undefined/string:undefined/boolean:false/boolean:false;inst lm1 count number:3;inst lm1 name string:b;inst lm1 n RefErr(n);inst lm1 modelData string:b;inst lm1 modelData.name undefined;inst lm1 index number:1;inst lm1 typeof model string:object;inst lm1 model.name string:b;inst lm1 model.n undefined;inst lm1 model.modelData string:b;inst lm1 model.modelData.name undefined;inst lm1 model.index number:1;inst lm1 model.nope undefined;inst lm1 outer string:outerprop;inst lm1 child string:b/number:1/string:b/string:object;inst lm1 outside undefined/undefined/undefined/string:undefined/boolean:false/boolean:false;rep num count number:3;rep num name RefErr(name);rep num n RefErr(n);rep num modelData number:1;rep num modelData.name undefined;rep num index number:1;rep num typeof model string:object;rep num model.name undefined;rep num model.n undefined;rep num model.modelData number:1;rep num model.modelData.name undefined;rep num model.index number:1;rep num model.nope undefined;rep num outer string:outerprop;rep num child RefErr(name)/number:1/number:1/string:object;rep num outside undefined/undefined/undefined/string:undefined/boolean:false/boolean:false;lv num count number:3;lv num name RefErr(name);lv num n RefErr(n);lv num modelData number:1;lv num modelData.name undefined;lv num index number:1;lv num typeof model string:object;lv num model.name undefined;lv num model.n undefined;lv num model.modelData number:1;lv num model.modelData.name undefined;lv num model.index number:1;lv num model.nope undefined;lv num outer string:outerprop;lv num child RefErr(name)/number:1/number:1/string:object;lv num outside undefined/undefined/undefined/string:undefined/boolean:false/boolean:false;gv num count number:3;gv num name RefErr(name);gv num n RefErr(n);gv num modelData number:1;gv num modelData.name undefined;gv num index number:1;gv num typeof model string:object;gv num model.name undefined;gv num model.n undefined;gv num model.modelData number:1;gv num model.modelData.name undefined;gv num model.index number:1;gv num model.nope undefined;gv num outer string:outerprop;gv num child RefErr(name)/number:1/number:1/string:object;gv num outside undefined/undefined/undefined/string:undefined/boolean:false/boolean:false;inst num count number:3;inst num name RefErr(name);inst num n RefErr(n);inst num modelData number:1;inst num modelData.name undefined;inst num index number:1;inst num typeof model string:object;inst num model.name undefined;inst num model.n undefined;inst num model.modelData number:1;inst num model.modelData.name undefined;inst num model.index number:1;inst num model.nope undefined;inst num outer string:outerprop;inst num child RefErr(name)/number:1/number:1/string:object;inst num outside undefined/undefined/undefined/string:undefined/boolean:false/boolean:false;rep arr count number:3;rep arr name RefErr(name);rep arr n RefErr(n);rep arr modelData string:y;rep arr modelData.name undefined;rep arr index number:1;rep arr typeof model string:object;rep arr model.name undefined;rep arr model.n undefined;rep arr model.modelData string:y;rep arr model.modelData.name undefined;rep arr model.index number:1;rep arr model.nope undefined;rep arr outer string:outerprop;rep arr child RefErr(name)/number:1/string:y/string:object;rep arr outside undefined/undefined/undefined/string:undefined/boolean:false/boolean:false;lv arr count number:3;lv arr name RefErr(name);lv arr n RefErr(n);lv arr modelData string:y;lv arr modelData.name undefined;lv arr index number:1;lv arr typeof model string:object;lv arr model.name undefined;lv arr model.n undefined;lv arr model.modelData string:y;lv arr model.modelData.name undefined;lv arr model.index number:1;lv arr model.nope undefined;lv arr outer string:outerprop;lv arr child RefErr(name)/number:1/string:y/string:object;lv arr outside undefined/undefined/undefined/string:undefined/boolean:false/boolean:false;gv arr count number:3;gv arr name RefErr(name);gv arr n RefErr(n);gv arr modelData string:y;gv arr modelData.name undefined;gv arr index number:1;gv arr typeof model string:object;gv arr model.name undefined;gv arr model.n undefined;gv arr model.modelData string:y;gv arr model.modelData.name undefined;gv arr model.index number:1;gv arr model.nope undefined;gv arr outer string:outerprop;gv arr child RefErr(name)/number:1/string:y/string:object;gv arr outside undefined/undefined/undefined/string:undefined/boolean:false/boolean:false;inst arr count number:3;inst arr name RefErr(name);inst arr n RefErr(n);inst arr modelData string:y;inst arr modelData.name undefined;inst arr index number:1;inst arr typeof model string:object;inst arr model.name undefined;inst arr model.n undefined;inst arr model.modelData string:y;inst arr model.modelData.name undefined;inst arr model.index number:1;inst arr model.nope undefined;inst arr outer string:outerprop;inst arr child RefErr(name)/number:1/string:y/string:object;inst arr outside undefined/undefined/undefined/string:undefined/boolean:false/boolean:false;rep arro count number:3;rep arro name RefErr(name);rep arro n RefErr(n);rep arro modelData object;rep arro modelData.name string:b;rep arro index number:1;rep arro typeof model string:object;rep arro model.name undefined;rep arro model.n undefined;rep arro model.modelData object;rep arro model.modelData.name string:b;rep arro model.index number:1;rep arro model.nope undefined;rep arro outer string:outerprop;rep arro child RefErr(name)/number:1/object/string:object;rep arro outside undefined/undefined/undefined/string:undefined/boolean:false/boolean:false;lv arro count number:3;lv arro name RefErr(name);lv arro n RefErr(n);lv arro modelData object;lv arro modelData.name string:b;lv arro index number:1;lv arro typeof model string:object;lv arro model.name undefined;lv arro model.n undefined;lv arro model.modelData object;lv arro model.modelData.name string:b;lv arro model.index number:1;lv arro model.nope undefined;lv arro outer string:outerprop;lv arro child RefErr(name)/number:1/object/string:object;lv arro outside undefined/undefined/undefined/string:undefined/boolean:false/boolean:false;gv arro count number:3;gv arro name RefErr(name);gv arro n RefErr(n);gv arro modelData object;gv arro modelData.name string:b;gv arro index number:1;gv arro typeof model string:object;gv arro model.name undefined;gv arro model.n undefined;gv arro model.modelData object;gv arro model.modelData.name string:b;gv arro model.index number:1;gv arro model.nope undefined;gv arro outer string:outerprop;gv arro child RefErr(name)/number:1/object/string:object;gv arro outside undefined/undefined/undefined/string:undefined/boolean:false/boolean:false;inst arro count number:3;inst arro name RefErr(name);inst arro n RefErr(n);inst arro modelData object;inst arro modelData.name string:b;inst arro index number:1;inst arro typeof model string:object;inst arro model.name undefined;inst arro model.n undefined;inst arro model.modelData object;inst arro model.modelData.name string:b;inst arro model.index number:1;inst arro model.nope undefined;inst arro outer string:outerprop;inst arro child RefErr(name)/number:1/object/string:object;inst arro outside undefined/undefined/undefined/string:undefined/boolean:false/boolean:false;rep objs count number:3;rep objs name string:b;rep objs n number:2;rep objs modelData object;rep objs modelData.name string:b;rep objs index number:1;rep objs typeof model string:object;rep objs model.name string:b;rep objs model.n number:2;rep objs model.modelData object;rep objs model.modelData.name string:b;rep objs model.index number:1;rep objs model.nope undefined;rep objs outer string:outerprop;rep objs child string:b/number:1/object/string:object;rep objs outside undefined/undefined/undefined/string:undefined/boolean:false/boolean:false;lv objs count number:3;lv objs name string:b;lv objs n number:2;lv objs modelData object;lv objs modelData.name string:b;lv objs index number:1;lv objs typeof model string:object;lv objs model.name string:b;lv objs model.n number:2;lv objs model.modelData object;lv objs model.modelData.name string:b;lv objs model.index number:1;lv objs model.nope undefined;lv objs outer string:outerprop;lv objs child string:b/number:1/object/string:object;lv objs outside undefined/undefined/undefined/string:undefined/boolean:false/boolean:false;gv objs count number:3;gv objs name string:b;gv objs n number:2;gv objs modelData object;gv objs modelData.name string:b;gv objs index number:1;gv objs typeof model string:object;gv objs model.name string:b;gv objs model.n number:2;gv objs model.modelData object;gv objs model.modelData.name string:b;gv objs model.index number:1;gv objs model.nope undefined;gv objs outer string:outerprop;gv objs child string:b/number:1/object/string:object;gv objs outside undefined/undefined/undefined/string:undefined/boolean:false/boolean:false;inst objs count number:3;inst objs name string:b;inst objs n number:2;inst objs modelData object;inst objs modelData.name string:b;inst objs index number:1;inst objs typeof model string:object;inst objs model.name string:b;inst objs model.n number:2;inst objs model.modelData object;inst objs model.modelData.name string:b;inst objs model.index number:1;inst objs model.nope undefined;inst objs outer string:outerprop;inst objs child string:b/number:1/object/string:object;inst objs outside undefined/undefined/undefined/string:undefined/boolean:false/boolean:false
import QtQuick
import QtQml.Models
Item {
    id: root
    width: 400; height: 400
    property string result: ""
    property var seen: []
    property string outer: "outerprop"
    function s(v) {
        if (v === undefined) return "undefined"
        if (v === null) return "null"
        if (typeof v === "object" || typeof v === "function") return typeof v
        return typeof v + ":" + v
    }
    function q(fn) { try { return s(fn()) } catch (e) { return ("" + e).replace(/ReferenceError: (\w+) is not defined/, "RefErr($1)") } }
    function t(name, fn) { seen.push(name + " " + q(fn)) }
    ListModel { id: lm; ListElement { name: "a"; n: 1 } ListElement { name: "b"; n: 2 } ListElement { name: "c"; n: 3 } }
    ListModel { id: lm1; ListElement { name: "a" } ListElement { name: "b" } ListElement { name: "c" } }
    property var arr: ["x", "y", "z"]
    property var arro: [{ name: "a", n: 1 }, { name: "b", n: 2 }, { name: "c", n: 3 }]
    property list<QtObject> objs: [
        QtObject { property string name: "a"; property int n: 1 },
        QtObject { property string name: "b"; property int n: 2 },
        QtObject { property string name: "c"; property int n: 3 }
    ]
    function outside(d) {
        return [q(() => d.name), q(() => d.index), q(() => d.modelData), q(() => typeof d.model), q(() => "name" in d), q(() => "index" in d)].join("/")
    }
    Repeater { id: rep_lm; model: lm; delegate: Item {
        id: d
        property string tag: "rep lm"
        width: 10; height: 10
        function reads() {
            root.t(tag + " name", () => name)
            root.t(tag + " n", () => n)
            root.t(tag + " modelData", () => modelData)
            root.t(tag + " modelData.name", () => modelData.name)
            root.t(tag + " index", () => index)
            root.t(tag + " typeof model", () => typeof model)
            root.t(tag + " model.name", () => model.name)
            root.t(tag + " model.n", () => model.n)
            root.t(tag + " model.modelData", () => model.modelData)
            root.t(tag + " model.modelData.name", () => model.modelData.name)
            root.t(tag + " model.index", () => model.index)
            root.t(tag + " model.nope", () => model.nope)
            root.t(tag + " outer", () => outer)
            seen.push(tag + " child " + kid.via())
            seen.push(tag + " outside " + root.outside(d))
        }
        Item {
            id: kid
            function via() { return [root.q(() => name), root.q(() => index), root.q(() => modelData), root.q(() => typeof model)].join("/") }
        }
    } }
    ListView { id: lv_lm; width: 100; height: 100; model: lm; delegate: Item {
        id: d
        property string tag: "lv lm"
        width: 10; height: 10
        function reads() {
            root.t(tag + " name", () => name)
            root.t(tag + " n", () => n)
            root.t(tag + " modelData", () => modelData)
            root.t(tag + " modelData.name", () => modelData.name)
            root.t(tag + " index", () => index)
            root.t(tag + " typeof model", () => typeof model)
            root.t(tag + " model.name", () => model.name)
            root.t(tag + " model.n", () => model.n)
            root.t(tag + " model.modelData", () => model.modelData)
            root.t(tag + " model.modelData.name", () => model.modelData.name)
            root.t(tag + " model.index", () => model.index)
            root.t(tag + " model.nope", () => model.nope)
            root.t(tag + " outer", () => outer)
            seen.push(tag + " child " + kid.via())
            seen.push(tag + " outside " + root.outside(d))
        }
        Item {
            id: kid
            function via() { return [root.q(() => name), root.q(() => index), root.q(() => modelData), root.q(() => typeof model)].join("/") }
        }
    } }
    GridView { id: gv_lm; width: 100; height: 100; model: lm; delegate: Item {
        id: d
        property string tag: "gv lm"
        width: 10; height: 10
        function reads() {
            root.t(tag + " name", () => name)
            root.t(tag + " n", () => n)
            root.t(tag + " modelData", () => modelData)
            root.t(tag + " modelData.name", () => modelData.name)
            root.t(tag + " index", () => index)
            root.t(tag + " typeof model", () => typeof model)
            root.t(tag + " model.name", () => model.name)
            root.t(tag + " model.n", () => model.n)
            root.t(tag + " model.modelData", () => model.modelData)
            root.t(tag + " model.modelData.name", () => model.modelData.name)
            root.t(tag + " model.index", () => model.index)
            root.t(tag + " model.nope", () => model.nope)
            root.t(tag + " outer", () => outer)
            seen.push(tag + " child " + kid.via())
            seen.push(tag + " outside " + root.outside(d))
        }
        Item {
            id: kid
            function via() { return [root.q(() => name), root.q(() => index), root.q(() => modelData), root.q(() => typeof model)].join("/") }
        }
    } }
    Instantiator { id: inst_lm; model: lm; delegate: Item {
        id: d
        property string tag: "inst lm"
        width: 10; height: 10
        function reads() {
            root.t(tag + " name", () => name)
            root.t(tag + " n", () => n)
            root.t(tag + " modelData", () => modelData)
            root.t(tag + " modelData.name", () => modelData.name)
            root.t(tag + " index", () => index)
            root.t(tag + " typeof model", () => typeof model)
            root.t(tag + " model.name", () => model.name)
            root.t(tag + " model.n", () => model.n)
            root.t(tag + " model.modelData", () => model.modelData)
            root.t(tag + " model.modelData.name", () => model.modelData.name)
            root.t(tag + " model.index", () => model.index)
            root.t(tag + " model.nope", () => model.nope)
            root.t(tag + " outer", () => outer)
            seen.push(tag + " child " + kid.via())
            seen.push(tag + " outside " + root.outside(d))
        }
        Item {
            id: kid
            function via() { return [root.q(() => name), root.q(() => index), root.q(() => modelData), root.q(() => typeof model)].join("/") }
        }
    } }
    Repeater { id: rep_lm1; model: lm1; delegate: Item {
        id: d
        property string tag: "rep lm1"
        width: 10; height: 10
        function reads() {
            root.t(tag + " name", () => name)
            root.t(tag + " n", () => n)
            root.t(tag + " modelData", () => modelData)
            root.t(tag + " modelData.name", () => modelData.name)
            root.t(tag + " index", () => index)
            root.t(tag + " typeof model", () => typeof model)
            root.t(tag + " model.name", () => model.name)
            root.t(tag + " model.n", () => model.n)
            root.t(tag + " model.modelData", () => model.modelData)
            root.t(tag + " model.modelData.name", () => model.modelData.name)
            root.t(tag + " model.index", () => model.index)
            root.t(tag + " model.nope", () => model.nope)
            root.t(tag + " outer", () => outer)
            seen.push(tag + " child " + kid.via())
            seen.push(tag + " outside " + root.outside(d))
        }
        Item {
            id: kid
            function via() { return [root.q(() => name), root.q(() => index), root.q(() => modelData), root.q(() => typeof model)].join("/") }
        }
    } }
    ListView { id: lv_lm1; width: 100; height: 100; model: lm1; delegate: Item {
        id: d
        property string tag: "lv lm1"
        width: 10; height: 10
        function reads() {
            root.t(tag + " name", () => name)
            root.t(tag + " n", () => n)
            root.t(tag + " modelData", () => modelData)
            root.t(tag + " modelData.name", () => modelData.name)
            root.t(tag + " index", () => index)
            root.t(tag + " typeof model", () => typeof model)
            root.t(tag + " model.name", () => model.name)
            root.t(tag + " model.n", () => model.n)
            root.t(tag + " model.modelData", () => model.modelData)
            root.t(tag + " model.modelData.name", () => model.modelData.name)
            root.t(tag + " model.index", () => model.index)
            root.t(tag + " model.nope", () => model.nope)
            root.t(tag + " outer", () => outer)
            seen.push(tag + " child " + kid.via())
            seen.push(tag + " outside " + root.outside(d))
        }
        Item {
            id: kid
            function via() { return [root.q(() => name), root.q(() => index), root.q(() => modelData), root.q(() => typeof model)].join("/") }
        }
    } }
    GridView { id: gv_lm1; width: 100; height: 100; model: lm1; delegate: Item {
        id: d
        property string tag: "gv lm1"
        width: 10; height: 10
        function reads() {
            root.t(tag + " name", () => name)
            root.t(tag + " n", () => n)
            root.t(tag + " modelData", () => modelData)
            root.t(tag + " modelData.name", () => modelData.name)
            root.t(tag + " index", () => index)
            root.t(tag + " typeof model", () => typeof model)
            root.t(tag + " model.name", () => model.name)
            root.t(tag + " model.n", () => model.n)
            root.t(tag + " model.modelData", () => model.modelData)
            root.t(tag + " model.modelData.name", () => model.modelData.name)
            root.t(tag + " model.index", () => model.index)
            root.t(tag + " model.nope", () => model.nope)
            root.t(tag + " outer", () => outer)
            seen.push(tag + " child " + kid.via())
            seen.push(tag + " outside " + root.outside(d))
        }
        Item {
            id: kid
            function via() { return [root.q(() => name), root.q(() => index), root.q(() => modelData), root.q(() => typeof model)].join("/") }
        }
    } }
    Instantiator { id: inst_lm1; model: lm1; delegate: Item {
        id: d
        property string tag: "inst lm1"
        width: 10; height: 10
        function reads() {
            root.t(tag + " name", () => name)
            root.t(tag + " n", () => n)
            root.t(tag + " modelData", () => modelData)
            root.t(tag + " modelData.name", () => modelData.name)
            root.t(tag + " index", () => index)
            root.t(tag + " typeof model", () => typeof model)
            root.t(tag + " model.name", () => model.name)
            root.t(tag + " model.n", () => model.n)
            root.t(tag + " model.modelData", () => model.modelData)
            root.t(tag + " model.modelData.name", () => model.modelData.name)
            root.t(tag + " model.index", () => model.index)
            root.t(tag + " model.nope", () => model.nope)
            root.t(tag + " outer", () => outer)
            seen.push(tag + " child " + kid.via())
            seen.push(tag + " outside " + root.outside(d))
        }
        Item {
            id: kid
            function via() { return [root.q(() => name), root.q(() => index), root.q(() => modelData), root.q(() => typeof model)].join("/") }
        }
    } }
    Repeater { id: rep_num; model: 3; delegate: Item {
        id: d
        property string tag: "rep num"
        width: 10; height: 10
        function reads() {
            root.t(tag + " name", () => name)
            root.t(tag + " n", () => n)
            root.t(tag + " modelData", () => modelData)
            root.t(tag + " modelData.name", () => modelData.name)
            root.t(tag + " index", () => index)
            root.t(tag + " typeof model", () => typeof model)
            root.t(tag + " model.name", () => model.name)
            root.t(tag + " model.n", () => model.n)
            root.t(tag + " model.modelData", () => model.modelData)
            root.t(tag + " model.modelData.name", () => model.modelData.name)
            root.t(tag + " model.index", () => model.index)
            root.t(tag + " model.nope", () => model.nope)
            root.t(tag + " outer", () => outer)
            seen.push(tag + " child " + kid.via())
            seen.push(tag + " outside " + root.outside(d))
        }
        Item {
            id: kid
            function via() { return [root.q(() => name), root.q(() => index), root.q(() => modelData), root.q(() => typeof model)].join("/") }
        }
    } }
    ListView { id: lv_num; width: 100; height: 100; model: 3; delegate: Item {
        id: d
        property string tag: "lv num"
        width: 10; height: 10
        function reads() {
            root.t(tag + " name", () => name)
            root.t(tag + " n", () => n)
            root.t(tag + " modelData", () => modelData)
            root.t(tag + " modelData.name", () => modelData.name)
            root.t(tag + " index", () => index)
            root.t(tag + " typeof model", () => typeof model)
            root.t(tag + " model.name", () => model.name)
            root.t(tag + " model.n", () => model.n)
            root.t(tag + " model.modelData", () => model.modelData)
            root.t(tag + " model.modelData.name", () => model.modelData.name)
            root.t(tag + " model.index", () => model.index)
            root.t(tag + " model.nope", () => model.nope)
            root.t(tag + " outer", () => outer)
            seen.push(tag + " child " + kid.via())
            seen.push(tag + " outside " + root.outside(d))
        }
        Item {
            id: kid
            function via() { return [root.q(() => name), root.q(() => index), root.q(() => modelData), root.q(() => typeof model)].join("/") }
        }
    } }
    GridView { id: gv_num; width: 100; height: 100; model: 3; delegate: Item {
        id: d
        property string tag: "gv num"
        width: 10; height: 10
        function reads() {
            root.t(tag + " name", () => name)
            root.t(tag + " n", () => n)
            root.t(tag + " modelData", () => modelData)
            root.t(tag + " modelData.name", () => modelData.name)
            root.t(tag + " index", () => index)
            root.t(tag + " typeof model", () => typeof model)
            root.t(tag + " model.name", () => model.name)
            root.t(tag + " model.n", () => model.n)
            root.t(tag + " model.modelData", () => model.modelData)
            root.t(tag + " model.modelData.name", () => model.modelData.name)
            root.t(tag + " model.index", () => model.index)
            root.t(tag + " model.nope", () => model.nope)
            root.t(tag + " outer", () => outer)
            seen.push(tag + " child " + kid.via())
            seen.push(tag + " outside " + root.outside(d))
        }
        Item {
            id: kid
            function via() { return [root.q(() => name), root.q(() => index), root.q(() => modelData), root.q(() => typeof model)].join("/") }
        }
    } }
    Instantiator { id: inst_num; model: 3; delegate: Item {
        id: d
        property string tag: "inst num"
        width: 10; height: 10
        function reads() {
            root.t(tag + " name", () => name)
            root.t(tag + " n", () => n)
            root.t(tag + " modelData", () => modelData)
            root.t(tag + " modelData.name", () => modelData.name)
            root.t(tag + " index", () => index)
            root.t(tag + " typeof model", () => typeof model)
            root.t(tag + " model.name", () => model.name)
            root.t(tag + " model.n", () => model.n)
            root.t(tag + " model.modelData", () => model.modelData)
            root.t(tag + " model.modelData.name", () => model.modelData.name)
            root.t(tag + " model.index", () => model.index)
            root.t(tag + " model.nope", () => model.nope)
            root.t(tag + " outer", () => outer)
            seen.push(tag + " child " + kid.via())
            seen.push(tag + " outside " + root.outside(d))
        }
        Item {
            id: kid
            function via() { return [root.q(() => name), root.q(() => index), root.q(() => modelData), root.q(() => typeof model)].join("/") }
        }
    } }
    Repeater { id: rep_arr; model: root.arr; delegate: Item {
        id: d
        property string tag: "rep arr"
        width: 10; height: 10
        function reads() {
            root.t(tag + " name", () => name)
            root.t(tag + " n", () => n)
            root.t(tag + " modelData", () => modelData)
            root.t(tag + " modelData.name", () => modelData.name)
            root.t(tag + " index", () => index)
            root.t(tag + " typeof model", () => typeof model)
            root.t(tag + " model.name", () => model.name)
            root.t(tag + " model.n", () => model.n)
            root.t(tag + " model.modelData", () => model.modelData)
            root.t(tag + " model.modelData.name", () => model.modelData.name)
            root.t(tag + " model.index", () => model.index)
            root.t(tag + " model.nope", () => model.nope)
            root.t(tag + " outer", () => outer)
            seen.push(tag + " child " + kid.via())
            seen.push(tag + " outside " + root.outside(d))
        }
        Item {
            id: kid
            function via() { return [root.q(() => name), root.q(() => index), root.q(() => modelData), root.q(() => typeof model)].join("/") }
        }
    } }
    ListView { id: lv_arr; width: 100; height: 100; model: root.arr; delegate: Item {
        id: d
        property string tag: "lv arr"
        width: 10; height: 10
        function reads() {
            root.t(tag + " name", () => name)
            root.t(tag + " n", () => n)
            root.t(tag + " modelData", () => modelData)
            root.t(tag + " modelData.name", () => modelData.name)
            root.t(tag + " index", () => index)
            root.t(tag + " typeof model", () => typeof model)
            root.t(tag + " model.name", () => model.name)
            root.t(tag + " model.n", () => model.n)
            root.t(tag + " model.modelData", () => model.modelData)
            root.t(tag + " model.modelData.name", () => model.modelData.name)
            root.t(tag + " model.index", () => model.index)
            root.t(tag + " model.nope", () => model.nope)
            root.t(tag + " outer", () => outer)
            seen.push(tag + " child " + kid.via())
            seen.push(tag + " outside " + root.outside(d))
        }
        Item {
            id: kid
            function via() { return [root.q(() => name), root.q(() => index), root.q(() => modelData), root.q(() => typeof model)].join("/") }
        }
    } }
    GridView { id: gv_arr; width: 100; height: 100; model: root.arr; delegate: Item {
        id: d
        property string tag: "gv arr"
        width: 10; height: 10
        function reads() {
            root.t(tag + " name", () => name)
            root.t(tag + " n", () => n)
            root.t(tag + " modelData", () => modelData)
            root.t(tag + " modelData.name", () => modelData.name)
            root.t(tag + " index", () => index)
            root.t(tag + " typeof model", () => typeof model)
            root.t(tag + " model.name", () => model.name)
            root.t(tag + " model.n", () => model.n)
            root.t(tag + " model.modelData", () => model.modelData)
            root.t(tag + " model.modelData.name", () => model.modelData.name)
            root.t(tag + " model.index", () => model.index)
            root.t(tag + " model.nope", () => model.nope)
            root.t(tag + " outer", () => outer)
            seen.push(tag + " child " + kid.via())
            seen.push(tag + " outside " + root.outside(d))
        }
        Item {
            id: kid
            function via() { return [root.q(() => name), root.q(() => index), root.q(() => modelData), root.q(() => typeof model)].join("/") }
        }
    } }
    Instantiator { id: inst_arr; model: root.arr; delegate: Item {
        id: d
        property string tag: "inst arr"
        width: 10; height: 10
        function reads() {
            root.t(tag + " name", () => name)
            root.t(tag + " n", () => n)
            root.t(tag + " modelData", () => modelData)
            root.t(tag + " modelData.name", () => modelData.name)
            root.t(tag + " index", () => index)
            root.t(tag + " typeof model", () => typeof model)
            root.t(tag + " model.name", () => model.name)
            root.t(tag + " model.n", () => model.n)
            root.t(tag + " model.modelData", () => model.modelData)
            root.t(tag + " model.modelData.name", () => model.modelData.name)
            root.t(tag + " model.index", () => model.index)
            root.t(tag + " model.nope", () => model.nope)
            root.t(tag + " outer", () => outer)
            seen.push(tag + " child " + kid.via())
            seen.push(tag + " outside " + root.outside(d))
        }
        Item {
            id: kid
            function via() { return [root.q(() => name), root.q(() => index), root.q(() => modelData), root.q(() => typeof model)].join("/") }
        }
    } }
    Repeater { id: rep_arro; model: root.arro; delegate: Item {
        id: d
        property string tag: "rep arro"
        width: 10; height: 10
        function reads() {
            root.t(tag + " name", () => name)
            root.t(tag + " n", () => n)
            root.t(tag + " modelData", () => modelData)
            root.t(tag + " modelData.name", () => modelData.name)
            root.t(tag + " index", () => index)
            root.t(tag + " typeof model", () => typeof model)
            root.t(tag + " model.name", () => model.name)
            root.t(tag + " model.n", () => model.n)
            root.t(tag + " model.modelData", () => model.modelData)
            root.t(tag + " model.modelData.name", () => model.modelData.name)
            root.t(tag + " model.index", () => model.index)
            root.t(tag + " model.nope", () => model.nope)
            root.t(tag + " outer", () => outer)
            seen.push(tag + " child " + kid.via())
            seen.push(tag + " outside " + root.outside(d))
        }
        Item {
            id: kid
            function via() { return [root.q(() => name), root.q(() => index), root.q(() => modelData), root.q(() => typeof model)].join("/") }
        }
    } }
    ListView { id: lv_arro; width: 100; height: 100; model: root.arro; delegate: Item {
        id: d
        property string tag: "lv arro"
        width: 10; height: 10
        function reads() {
            root.t(tag + " name", () => name)
            root.t(tag + " n", () => n)
            root.t(tag + " modelData", () => modelData)
            root.t(tag + " modelData.name", () => modelData.name)
            root.t(tag + " index", () => index)
            root.t(tag + " typeof model", () => typeof model)
            root.t(tag + " model.name", () => model.name)
            root.t(tag + " model.n", () => model.n)
            root.t(tag + " model.modelData", () => model.modelData)
            root.t(tag + " model.modelData.name", () => model.modelData.name)
            root.t(tag + " model.index", () => model.index)
            root.t(tag + " model.nope", () => model.nope)
            root.t(tag + " outer", () => outer)
            seen.push(tag + " child " + kid.via())
            seen.push(tag + " outside " + root.outside(d))
        }
        Item {
            id: kid
            function via() { return [root.q(() => name), root.q(() => index), root.q(() => modelData), root.q(() => typeof model)].join("/") }
        }
    } }
    GridView { id: gv_arro; width: 100; height: 100; model: root.arro; delegate: Item {
        id: d
        property string tag: "gv arro"
        width: 10; height: 10
        function reads() {
            root.t(tag + " name", () => name)
            root.t(tag + " n", () => n)
            root.t(tag + " modelData", () => modelData)
            root.t(tag + " modelData.name", () => modelData.name)
            root.t(tag + " index", () => index)
            root.t(tag + " typeof model", () => typeof model)
            root.t(tag + " model.name", () => model.name)
            root.t(tag + " model.n", () => model.n)
            root.t(tag + " model.modelData", () => model.modelData)
            root.t(tag + " model.modelData.name", () => model.modelData.name)
            root.t(tag + " model.index", () => model.index)
            root.t(tag + " model.nope", () => model.nope)
            root.t(tag + " outer", () => outer)
            seen.push(tag + " child " + kid.via())
            seen.push(tag + " outside " + root.outside(d))
        }
        Item {
            id: kid
            function via() { return [root.q(() => name), root.q(() => index), root.q(() => modelData), root.q(() => typeof model)].join("/") }
        }
    } }
    Instantiator { id: inst_arro; model: root.arro; delegate: Item {
        id: d
        property string tag: "inst arro"
        width: 10; height: 10
        function reads() {
            root.t(tag + " name", () => name)
            root.t(tag + " n", () => n)
            root.t(tag + " modelData", () => modelData)
            root.t(tag + " modelData.name", () => modelData.name)
            root.t(tag + " index", () => index)
            root.t(tag + " typeof model", () => typeof model)
            root.t(tag + " model.name", () => model.name)
            root.t(tag + " model.n", () => model.n)
            root.t(tag + " model.modelData", () => model.modelData)
            root.t(tag + " model.modelData.name", () => model.modelData.name)
            root.t(tag + " model.index", () => model.index)
            root.t(tag + " model.nope", () => model.nope)
            root.t(tag + " outer", () => outer)
            seen.push(tag + " child " + kid.via())
            seen.push(tag + " outside " + root.outside(d))
        }
        Item {
            id: kid
            function via() { return [root.q(() => name), root.q(() => index), root.q(() => modelData), root.q(() => typeof model)].join("/") }
        }
    } }
    Repeater { id: rep_objs; model: root.objs; delegate: Item {
        id: d
        property string tag: "rep objs"
        width: 10; height: 10
        function reads() {
            root.t(tag + " name", () => name)
            root.t(tag + " n", () => n)
            root.t(tag + " modelData", () => modelData)
            root.t(tag + " modelData.name", () => modelData.name)
            root.t(tag + " index", () => index)
            root.t(tag + " typeof model", () => typeof model)
            root.t(tag + " model.name", () => model.name)
            root.t(tag + " model.n", () => model.n)
            root.t(tag + " model.modelData", () => model.modelData)
            root.t(tag + " model.modelData.name", () => model.modelData.name)
            root.t(tag + " model.index", () => model.index)
            root.t(tag + " model.nope", () => model.nope)
            root.t(tag + " outer", () => outer)
            seen.push(tag + " child " + kid.via())
            seen.push(tag + " outside " + root.outside(d))
        }
        Item {
            id: kid
            function via() { return [root.q(() => name), root.q(() => index), root.q(() => modelData), root.q(() => typeof model)].join("/") }
        }
    } }
    ListView { id: lv_objs; width: 100; height: 100; model: root.objs; delegate: Item {
        id: d
        property string tag: "lv objs"
        width: 10; height: 10
        function reads() {
            root.t(tag + " name", () => name)
            root.t(tag + " n", () => n)
            root.t(tag + " modelData", () => modelData)
            root.t(tag + " modelData.name", () => modelData.name)
            root.t(tag + " index", () => index)
            root.t(tag + " typeof model", () => typeof model)
            root.t(tag + " model.name", () => model.name)
            root.t(tag + " model.n", () => model.n)
            root.t(tag + " model.modelData", () => model.modelData)
            root.t(tag + " model.modelData.name", () => model.modelData.name)
            root.t(tag + " model.index", () => model.index)
            root.t(tag + " model.nope", () => model.nope)
            root.t(tag + " outer", () => outer)
            seen.push(tag + " child " + kid.via())
            seen.push(tag + " outside " + root.outside(d))
        }
        Item {
            id: kid
            function via() { return [root.q(() => name), root.q(() => index), root.q(() => modelData), root.q(() => typeof model)].join("/") }
        }
    } }
    GridView { id: gv_objs; width: 100; height: 100; model: root.objs; delegate: Item {
        id: d
        property string tag: "gv objs"
        width: 10; height: 10
        function reads() {
            root.t(tag + " name", () => name)
            root.t(tag + " n", () => n)
            root.t(tag + " modelData", () => modelData)
            root.t(tag + " modelData.name", () => modelData.name)
            root.t(tag + " index", () => index)
            root.t(tag + " typeof model", () => typeof model)
            root.t(tag + " model.name", () => model.name)
            root.t(tag + " model.n", () => model.n)
            root.t(tag + " model.modelData", () => model.modelData)
            root.t(tag + " model.modelData.name", () => model.modelData.name)
            root.t(tag + " model.index", () => model.index)
            root.t(tag + " model.nope", () => model.nope)
            root.t(tag + " outer", () => outer)
            seen.push(tag + " child " + kid.via())
            seen.push(tag + " outside " + root.outside(d))
        }
        Item {
            id: kid
            function via() { return [root.q(() => name), root.q(() => index), root.q(() => modelData), root.q(() => typeof model)].join("/") }
        }
    } }
    Instantiator { id: inst_objs; model: root.objs; delegate: Item {
        id: d
        property string tag: "inst objs"
        width: 10; height: 10
        function reads() {
            root.t(tag + " name", () => name)
            root.t(tag + " n", () => n)
            root.t(tag + " modelData", () => modelData)
            root.t(tag + " modelData.name", () => modelData.name)
            root.t(tag + " index", () => index)
            root.t(tag + " typeof model", () => typeof model)
            root.t(tag + " model.name", () => model.name)
            root.t(tag + " model.n", () => model.n)
            root.t(tag + " model.modelData", () => model.modelData)
            root.t(tag + " model.modelData.name", () => model.modelData.name)
            root.t(tag + " model.index", () => model.index)
            root.t(tag + " model.nope", () => model.nope)
            root.t(tag + " outer", () => outer)
            seen.push(tag + " child " + kid.via())
            seen.push(tag + " outside " + root.outside(d))
        }
        Item {
            id: kid
            function via() { return [root.q(() => name), root.q(() => index), root.q(() => modelData), root.q(() => typeof model)].join("/") }
        }
    } }
    function run() {
        t("rep lm count", () => rep_lm.count)
        try { rep_lm.itemAt(1).reads() } catch (e) { seen.push("rep lm FAILED " + e) }
        t("lv lm count", () => lv_lm.count)
        try { lv_lm.itemAtIndex(1).reads() } catch (e) { seen.push("lv lm FAILED " + e) }
        t("gv lm count", () => gv_lm.count)
        try { gv_lm.itemAtIndex(1).reads() } catch (e) { seen.push("gv lm FAILED " + e) }
        t("inst lm count", () => inst_lm.count)
        try { inst_lm.objectAt(1).reads() } catch (e) { seen.push("inst lm FAILED " + e) }
        t("rep lm1 count", () => rep_lm1.count)
        try { rep_lm1.itemAt(1).reads() } catch (e) { seen.push("rep lm1 FAILED " + e) }
        t("lv lm1 count", () => lv_lm1.count)
        try { lv_lm1.itemAtIndex(1).reads() } catch (e) { seen.push("lv lm1 FAILED " + e) }
        t("gv lm1 count", () => gv_lm1.count)
        try { gv_lm1.itemAtIndex(1).reads() } catch (e) { seen.push("gv lm1 FAILED " + e) }
        t("inst lm1 count", () => inst_lm1.count)
        try { inst_lm1.objectAt(1).reads() } catch (e) { seen.push("inst lm1 FAILED " + e) }
        t("rep num count", () => rep_num.count)
        try { rep_num.itemAt(1).reads() } catch (e) { seen.push("rep num FAILED " + e) }
        t("lv num count", () => lv_num.count)
        try { lv_num.itemAtIndex(1).reads() } catch (e) { seen.push("lv num FAILED " + e) }
        t("gv num count", () => gv_num.count)
        try { gv_num.itemAtIndex(1).reads() } catch (e) { seen.push("gv num FAILED " + e) }
        t("inst num count", () => inst_num.count)
        try { inst_num.objectAt(1).reads() } catch (e) { seen.push("inst num FAILED " + e) }
        t("rep arr count", () => rep_arr.count)
        try { rep_arr.itemAt(1).reads() } catch (e) { seen.push("rep arr FAILED " + e) }
        t("lv arr count", () => lv_arr.count)
        try { lv_arr.itemAtIndex(1).reads() } catch (e) { seen.push("lv arr FAILED " + e) }
        t("gv arr count", () => gv_arr.count)
        try { gv_arr.itemAtIndex(1).reads() } catch (e) { seen.push("gv arr FAILED " + e) }
        t("inst arr count", () => inst_arr.count)
        try { inst_arr.objectAt(1).reads() } catch (e) { seen.push("inst arr FAILED " + e) }
        t("rep arro count", () => rep_arro.count)
        try { rep_arro.itemAt(1).reads() } catch (e) { seen.push("rep arro FAILED " + e) }
        t("lv arro count", () => lv_arro.count)
        try { lv_arro.itemAtIndex(1).reads() } catch (e) { seen.push("lv arro FAILED " + e) }
        t("gv arro count", () => gv_arro.count)
        try { gv_arro.itemAtIndex(1).reads() } catch (e) { seen.push("gv arro FAILED " + e) }
        t("inst arro count", () => inst_arro.count)
        try { inst_arro.objectAt(1).reads() } catch (e) { seen.push("inst arro FAILED " + e) }
        t("rep objs count", () => rep_objs.count)
        try { rep_objs.itemAt(1).reads() } catch (e) { seen.push("rep objs FAILED " + e) }
        t("lv objs count", () => lv_objs.count)
        try { lv_objs.itemAtIndex(1).reads() } catch (e) { seen.push("lv objs FAILED " + e) }
        t("gv objs count", () => gv_objs.count)
        try { gv_objs.itemAtIndex(1).reads() } catch (e) { seen.push("gv objs FAILED " + e) }
        t("inst objs count", () => inst_objs.count)
        try { inst_objs.objectAt(1).reads() } catch (e) { seen.push("inst objs FAILED " + e) }
        result = seen.join(";")
    }
    Component.onCompleted: Qt.callLater(run)
}
