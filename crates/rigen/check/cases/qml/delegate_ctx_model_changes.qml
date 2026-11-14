// A delegate after its model changed (Qt 6.12): setProperty, set, get(i).role = v, append,
// insert, move, remove and clear of a ListModel keep the delegates of the rows that stay and
// tell only the bindings on what changed; a new number or a new array makes the delegates
// anew; a property of an object of a list of objects is read live. The first number of a row
// is the delegate's serial.
// qt: rep lm start [1:a,a,1,undefined,0/0 2:b,b,2,undefined,1/1 3:c,c,3,undefined,2/2] 3/3/3/3/3;rep lm setProperty [1:a,a,1,undefined,0/0 2:B,B,2,undefined,1/1 3:c,c,3,undefined,2/2] 4/4/3/4/3;rep lm set two [1:a,a,1,undefined,0/0 2:bb,bb,22,undefined,1/1 3:c,c,3,undefined,2/2] 5/5/4/5/3;rep lm set same [1:a,a,1,undefined,0/0 2:bb,bb,22,undefined,1/1 3:c,c,3,undefined,2/2] 6/6/4/6/3;rep lm set new role [1:a,a,1,undefined,0/0 2:bb,bb,22,undefined,1/1 3:c,c,3,undefined,2/2] 6/6/4/7/3;rep lm get().name= [1:a,a,1,undefined,0/0 2:bb,bb,22,undefined,1/1 3:C,C,3,undefined,2/2] 7/7/4/8/3;rep lm append [1:a,a,1,undefined,0/0 2:bb,bb,22,undefined,1/1 3:C,C,3,undefined,2/2 4:d,d,4,undefined,3/3] 8/8/5/9/4;rep lm insert 0 [5:z,z,0,undefined,0/0 1:a,a,1,undefined,1/1 2:bb,bb,22,undefined,2/2 3:C,C,3,undefined,3/3 4:d,d,4,undefined,4/4] 9/9/6/10/9;rep lm move 0->2 [1:a,a,1,undefined,0/0 2:bb,bb,22,undefined,1/1 5:z,z,0,undefined,2/2 3:C,C,3,undefined,3/3 4:d,d,4,undefined,4/4] 9/9/6/10/12;rep lm remove 1 [1:a,a,1,undefined,0/0 5:z,z,0,undefined,1/1 3:C,C,3,undefined,2/2 4:d,d,4,undefined,3/3] 9/9/6/10/16;rep lm set past end [1:a,a,1,undefined,0/0 5:z,z,0,undefined,1/1 3:C,C,3,undefined,2/2 4:d,d,4,undefined,3/3 6:e,e,5,undefined,4/4] 10/10/7/11/17;rep lm clear [] 10/10/7/11/22;rep lm append after clear [7:f,f,6,undefined,0/0] 11/11/8/12/23;rep num start [1:E,undefined,E,number:0,0/0 2:E,undefined,E,number:1,1/1] 2/2/2/2/2;rep num 3 [3:E,undefined,E,number:0,0/0 4:E,undefined,E,number:1,1/1 5:E,undefined,E,number:2,2/2] 5/5/5/5/5;rep num 1 [6:E,undefined,E,number:0,0/0] 6/6/6/6/6;rep num same [6:E,undefined,E,number:0,0/0] 6/6/6/6/6;rep arro start [1:E,undefined,E,object.a,0/0 2:E,undefined,E,object.b,1/1] 2/2/2/2/2;rep arro element.name= [1:E,undefined,E,object.a,0/0 2:E,undefined,E,object.b,1/1] 2/2/2/2/2;rep arro push [1:E,undefined,E,object.a,0/0 2:E,undefined,E,object.b,1/1] 2/2/2/2/2;rep arro reassign same content [3:E,undefined,E,object.a,0/0 4:E,undefined,E,object.B,1/1] 4/4/4/4/4;rep arro reassign longer [5:E,undefined,E,object.a,0/0 6:E,undefined,E,object.x,1/1 7:E,undefined,E,object.y,2/2] 7/7/7/7/7;rep arro reassign shorter [8:E,undefined,E,object.q,0/0] 8/8/8/8/8;rep objs start [1:a,a,1,object.a,0/0 2:b,b,2,object.b,1/1] 2/2/2/2/2;rep objs object.name= [1:a,a,1,object.a,0/0 2:B,B,2,object.B,1/1] 3/3/2/3/2;rep objs object.n= [1:a,a,1,object.a,0/0 2:B,B,22,object.B,1/1] 3/3/3/3/2;rep objs push [3:a,a,1,object.a,0/0 4:B,B,22,object.B,1/1 5:s,s,7,object.s,2/2] 6/6/6/6/5;rep objs reassign [6:s,s,7,object.s,0/0] 7/7/7/7/6;lv lm start [1:a,a,1,undefined,0/0 2:b,b,2,undefined,1/1 3:c,c,3,undefined,2/2] 3/3/3/3/3;lv lm setProperty [1:a,a,1,undefined,0/0 2:B,B,2,undefined,1/1 3:c,c,3,undefined,2/2] 4/4/3/4/3;lv lm set two [1:a,a,1,undefined,0/0 2:bb,bb,22,undefined,1/1 3:c,c,3,undefined,2/2] 5/5/4/5/3;lv lm set same [1:a,a,1,undefined,0/0 2:bb,bb,22,undefined,1/1 3:c,c,3,undefined,2/2] 6/6/4/6/3;lv lm set new role [1:a,a,1,undefined,0/0 2:bb,bb,22,undefined,1/1 3:c,c,3,undefined,2/2] 6/6/4/7/3;lv lm get().name= [1:a,a,1,undefined,0/0 2:bb,bb,22,undefined,1/1 3:C,C,3,undefined,2/2] 7/7/4/8/3;lv lm append [1:a,a,1,undefined,0/0 2:bb,bb,22,undefined,1/1 3:C,C,3,undefined,2/2 4:d,d,4,undefined,3/3] 8/8/5/9/4;lv lm insert 0 [5:z,z,0,undefined,0/0 1:a,a,1,undefined,1/1 2:bb,bb,22,undefined,2/2 3:C,C,3,undefined,3/3 4:d,d,4,undefined,4/4] 9/9/6/10/9;lv lm move 0->2 [1:a,a,1,undefined,0/0 2:bb,bb,22,undefined,1/1 5:z,z,0,undefined,2/2 3:C,C,3,undefined,3/3 4:d,d,4,undefined,4/4] 9/9/6/10/12;lv lm remove 1 [1:a,a,1,undefined,0/0 5:z,z,0,undefined,1/1 3:C,C,3,undefined,2/2 4:d,d,4,undefined,3/3] 9/9/6/10/16;lv lm set past end [1:a,a,1,undefined,0/0 5:z,z,0,undefined,1/1 3:C,C,3,undefined,2/2 4:d,d,4,undefined,3/3 6:e,e,5,undefined,4/4] 10/10/7/11/17;lv lm clear [] 10/10/7/11/22;lv lm append after clear [7:f,f,6,undefined,0/0] 11/11/8/12/23;lv num start [1:E,undefined,E,number:0,0/0 2:E,undefined,E,number:1,1/1] 2/2/2/2/2;lv num 3 [3:E,undefined,E,number:0,0/0 4:E,undefined,E,number:1,1/1 5:E,undefined,E,number:2,2/2] 5/5/5/5/5;lv num 1 [6:E,undefined,E,number:0,0/0] 6/6/6/6/6;lv num same [6:E,undefined,E,number:0,0/0] 6/6/6/6/6;lv arro start [1:E,undefined,E,object.a,0/0 2:E,undefined,E,object.b,1/1] 2/2/2/2/2;lv arro element.name= [1:E,undefined,E,object.a,0/0 2:E,undefined,E,object.b,1/1] 2/2/2/2/2;lv arro reassign same content [3:E,undefined,E,object.a,0/0 4:E,undefined,E,object.B,1/1] 4/4/4/4/4;lv arro reassign longer [5:E,undefined,E,object.a,0/0 6:E,undefined,E,object.x,1/1 7:E,undefined,E,object.y,2/2] 7/7/7/7/7;lv arro reassign shorter [8:E,undefined,E,object.q,0/0] 8/8/8/8/8;lv objs start [1:a,a,1,object.a,0/0 2:b,b,2,object.b,1/1] 2/2/2/2/2;lv objs object.name= [1:a,a,1,object.a,0/0 2:B,B,2,object.B,1/1] 3/3/2/3/2;lv objs object.n= [1:a,a,1,object.a,0/0 2:B,B,22,object.B,1/1] 3/3/3/3/2;lv objs push [3:a,a,1,object.a,0/0 4:B,B,22,object.B,1/1 5:s,s,7,object.s,2/2] 6/6/6/6/5;lv objs reassign [6:s,s,7,object.s,0/0] 7/7/7/7/6;gv lm start [1:a,a,1,undefined,0/0 2:b,b,2,undefined,1/1 3:c,c,3,undefined,2/2] 3/3/3/3/3;gv lm setProperty [1:a,a,1,undefined,0/0 2:B,B,2,undefined,1/1 3:c,c,3,undefined,2/2] 4/4/3/4/3;gv lm set two [1:a,a,1,undefined,0/0 2:bb,bb,22,undefined,1/1 3:c,c,3,undefined,2/2] 5/5/4/5/3;gv lm set same [1:a,a,1,undefined,0/0 2:bb,bb,22,undefined,1/1 3:c,c,3,undefined,2/2] 6/6/4/6/3;gv lm set new role [1:a,a,1,undefined,0/0 2:bb,bb,22,undefined,1/1 3:c,c,3,undefined,2/2] 6/6/4/7/3;gv lm get().name= [1:a,a,1,undefined,0/0 2:bb,bb,22,undefined,1/1 3:C,C,3,undefined,2/2] 7/7/4/8/3;gv lm append [1:a,a,1,undefined,0/0 2:bb,bb,22,undefined,1/1 3:C,C,3,undefined,2/2 4:d,d,4,undefined,3/3] 8/8/5/9/4;gv lm insert 0 [5:z,z,0,undefined,0/0 1:a,a,1,undefined,1/1 2:bb,bb,22,undefined,2/2 3:C,C,3,undefined,3/3 4:d,d,4,undefined,4/4] 9/9/6/10/9;gv lm move 0->2 [1:a,a,1,undefined,0/0 2:bb,bb,22,undefined,1/1 5:z,z,0,undefined,2/2 3:C,C,3,undefined,3/3 4:d,d,4,undefined,4/4] 9/9/6/10/12;gv lm remove 1 [1:a,a,1,undefined,0/0 5:z,z,0,undefined,1/1 3:C,C,3,undefined,2/2 4:d,d,4,undefined,3/3] 9/9/6/10/16;gv lm set past end [1:a,a,1,undefined,0/0 5:z,z,0,undefined,1/1 3:C,C,3,undefined,2/2 4:d,d,4,undefined,3/3 6:e,e,5,undefined,4/4] 10/10/7/11/17;gv lm clear [] 10/10/7/11/22;gv lm append after clear [7:f,f,6,undefined,0/0] 11/11/8/12/23;gv num start [1:E,undefined,E,number:0,0/0 2:E,undefined,E,number:1,1/1] 2/2/2/2/2;gv num 3 [3:E,undefined,E,number:0,0/0 4:E,undefined,E,number:1,1/1 5:E,undefined,E,number:2,2/2] 5/5/5/5/5;gv num 1 [6:E,undefined,E,number:0,0/0] 6/6/6/6/6;gv num same [6:E,undefined,E,number:0,0/0] 6/6/6/6/6;gv arro start [1:E,undefined,E,object.a,0/0 2:E,undefined,E,object.b,1/1] 2/2/2/2/2;gv arro element.name= [1:E,undefined,E,object.a,0/0 2:E,undefined,E,object.b,1/1] 2/2/2/2/2;gv arro reassign same content [3:E,undefined,E,object.a,0/0 4:E,undefined,E,object.B,1/1] 4/4/4/4/4;gv arro reassign longer [5:E,undefined,E,object.a,0/0 6:E,undefined,E,object.x,1/1 7:E,undefined,E,object.y,2/2] 7/7/7/7/7;gv arro reassign shorter [8:E,undefined,E,object.q,0/0] 8/8/8/8/8;gv objs start [1:a,a,1,object.a,0/0 2:b,b,2,object.b,1/1] 2/2/2/2/2;gv objs object.name= [1:a,a,1,object.a,0/0 2:B,B,2,object.B,1/1] 3/3/2/3/2;gv objs object.n= [1:a,a,1,object.a,0/0 2:B,B,22,object.B,1/1] 3/3/3/3/2;gv objs push [3:a,a,1,object.a,0/0 4:B,B,22,object.B,1/1 5:s,s,7,object.s,2/2] 6/6/6/6/5;gv objs reassign [6:s,s,7,object.s,0/0] 7/7/7/7/6;inst lm start [1:a,a,1,undefined,0/0 2:b,b,2,undefined,1/1 3:c,c,3,undefined,2/2] 3/3/3/3/3;inst lm setProperty [1:a,a,1,undefined,0/0 2:B,B,2,undefined,1/1 3:c,c,3,undefined,2/2] 4/4/3/4/3;inst lm set two [1:a,a,1,undefined,0/0 2:bb,bb,22,undefined,1/1 3:c,c,3,undefined,2/2] 5/5/4/5/3;inst lm set same [1:a,a,1,undefined,0/0 2:bb,bb,22,undefined,1/1 3:c,c,3,undefined,2/2] 6/6/4/6/3;inst lm set new role [1:a,a,1,undefined,0/0 2:bb,bb,22,undefined,1/1 3:c,c,3,undefined,2/2] 6/6/4/7/3;inst lm get().name= [1:a,a,1,undefined,0/0 2:bb,bb,22,undefined,1/1 3:C,C,3,undefined,2/2] 7/7/4/8/3;inst num start [1:E,undefined,E,number:0,0/0 2:E,undefined,E,number:1,1/1] 2/2/2/2/2;inst num 3 [3:E,undefined,E,number:0,0/0 4:E,undefined,E,number:1,1/1 5:E,undefined,E,number:2,2/2] 5/5/5/5/5;inst num 1 [6:E,undefined,E,number:0,0/0] 6/6/6/6/6;inst num same [6:E,undefined,E,number:0,0/0] 6/6/6/6/6;inst arro start [1:E,undefined,E,object.a,0/0 2:E,undefined,E,object.b,1/1] 2/2/2/2/2;inst arro element.name= [1:E,undefined,E,object.a,0/0 2:E,undefined,E,object.b,1/1] 2/2/2/2/2;inst arro push [1:E,undefined,E,object.a,0/0 2:E,undefined,E,object.b,1/1] 2/2/2/2/2;inst arro reassign same content [3:E,undefined,E,object.a,0/0 4:E,undefined,E,object.B,1/1] 4/4/4/4/4;inst arro reassign longer [5:E,undefined,E,object.a,0/0 6:E,undefined,E,object.x,1/1 7:E,undefined,E,object.y,2/2] 7/7/7/7/7;inst arro reassign shorter [8:E,undefined,E,object.q,0/0] 8/8/8/8/8;inst objs start [1:a,a,1,object.a,0/0 2:b,b,2,object.b,1/1] 2/2/2/2/2;inst objs object.name= [1:a,a,1,object.a,0/0 2:B,B,2,object.B,1/1] 3/3/2/3/2;inst objs object.n= [1:a,a,1,object.a,0/0 2:B,B,22,object.B,1/1] 3/3/3/3/2;inst objs push [3:a,a,1,object.a,0/0 4:B,B,22,object.B,1/1 5:s,s,7,object.s,2/2] 6/6/6/6/5;inst objs reassign [6:s,s,7,object.s,0/0] 7/7/7/7/6
import QtQuick
import QtQml.Models
Item {
    id: root
    width: 400; height: 400
    property string result: ""
    property var seen: []
    property var cnt: ({})
    property var births: ({})
    function born(tag) { births[tag] = (births[tag] || 0) + 1; return births[tag] }
    function bump(k) { cnt[k] = (cnt[k] || 0) + 1 }
    function counts(tag) { return [cnt[tag + " name"], cnt[tag + " mname"], cnt[tag + " n"], cnt[tag + " md"], cnt[tag + " index"]].join("/") }
    function s(v) {
        if (v === undefined) return "undefined"
        if (v === null) return "null"
        if (typeof v === "object" || typeof v === "function") return typeof v
        return typeof v + ":" + v
    }
    function dump(tag, view, at, label) {
        if (view.forceLayout) view.forceLayout()
        var a = []
        for (var i = 0; i < view.count; i++) { var o = view[at](i); a.push(o ? o.st() : "null") }
        seen.push(tag + " " + label + " [" + a.join(" ") + "] " + counts(tag))
    }
    function step(tag, view, at, label, fn) {
        var e = ""
        try { fn() } catch (x) { e = " " + x }
        dump(tag, view, at, label + e)
    }
    ListModel { id: lm_rep; ListElement { name: "a"; n: 1 } ListElement { name: "b"; n: 2 } ListElement { name: "c"; n: 3 } }
    Repeater { id: rep_lm; model: lm_rep; delegate: Item {
        id: d
        width: 10; height: 10
        property int born: root.born("rep lm")
        property var bName: { root.bump("rep lm name"); try { return name } catch (e) { return "E" } }
        property var bMName: { root.bump("rep lm mname"); try { return model.name } catch (e) { return "E" } }
        property var bN: { root.bump("rep lm n"); try { return n } catch (e) { return "E" } }
        property var bMd: { root.bump("rep lm md"); try { return root.s(modelData) + (modelData && modelData.name ? "." + modelData.name : "") } catch (e) { return "E" } }
        property var bIndex: { root.bump("rep lm index"); try { return index + "/" + model.index } catch (e) { return "E" } }
        function st() { return born + ":" + bName + "," + bMName + "," + bN + "," + bMd + "," + bIndex }
    } }
    property int num_rep: 2
    Repeater { id: rep_num; model: root.num_rep; delegate: Item {
        id: d
        width: 10; height: 10
        property int born: root.born("rep num")
        property var bName: { root.bump("rep num name"); try { return name } catch (e) { return "E" } }
        property var bMName: { root.bump("rep num mname"); try { return model.name } catch (e) { return "E" } }
        property var bN: { root.bump("rep num n"); try { return n } catch (e) { return "E" } }
        property var bMd: { root.bump("rep num md"); try { return root.s(modelData) + (modelData && modelData.name ? "." + modelData.name : "") } catch (e) { return "E" } }
        property var bIndex: { root.bump("rep num index"); try { return index + "/" + model.index } catch (e) { return "E" } }
        function st() { return born + ":" + bName + "," + bMName + "," + bN + "," + bMd + "," + bIndex }
    } }
    property var arro_rep: [{ name: "a", n: 1 }, { name: "b", n: 2 }]
    Repeater { id: rep_arro; model: root.arro_rep; delegate: Item {
        id: d
        width: 10; height: 10
        property int born: root.born("rep arro")
        property var bName: { root.bump("rep arro name"); try { return name } catch (e) { return "E" } }
        property var bMName: { root.bump("rep arro mname"); try { return model.name } catch (e) { return "E" } }
        property var bN: { root.bump("rep arro n"); try { return n } catch (e) { return "E" } }
        property var bMd: { root.bump("rep arro md"); try { return root.s(modelData) + (modelData && modelData.name ? "." + modelData.name : "") } catch (e) { return "E" } }
        property var bIndex: { root.bump("rep arro index"); try { return index + "/" + model.index } catch (e) { return "E" } }
        function st() { return born + ":" + bName + "," + bMName + "," + bN + "," + bMd + "," + bIndex }
    } }
    property list<QtObject> objs_rep: [ QtObject { property string name: "a"; property int n: 1 }, QtObject { property string name: "b"; property int n: 2 } ]
    QtObject { id: spare_rep; property string name: "s"; property int n: 7 }
    Repeater { id: rep_objs; model: root.objs_rep; delegate: Item {
        id: d
        width: 10; height: 10
        property int born: root.born("rep objs")
        property var bName: { root.bump("rep objs name"); try { return name } catch (e) { return "E" } }
        property var bMName: { root.bump("rep objs mname"); try { return model.name } catch (e) { return "E" } }
        property var bN: { root.bump("rep objs n"); try { return n } catch (e) { return "E" } }
        property var bMd: { root.bump("rep objs md"); try { return root.s(modelData) + (modelData && modelData.name ? "." + modelData.name : "") } catch (e) { return "E" } }
        property var bIndex: { root.bump("rep objs index"); try { return index + "/" + model.index } catch (e) { return "E" } }
        function st() { return born + ":" + bName + "," + bMName + "," + bN + "," + bMd + "," + bIndex }
    } }
    ListModel { id: lm_lv; ListElement { name: "a"; n: 1 } ListElement { name: "b"; n: 2 } ListElement { name: "c"; n: 3 } }
    ListView { id: lv_lm; width: 100; height: 100; model: lm_lv; delegate: Item {
        id: d
        width: 10; height: 10
        property int born: root.born("lv lm")
        property var bName: { root.bump("lv lm name"); try { return name } catch (e) { return "E" } }
        property var bMName: { root.bump("lv lm mname"); try { return model.name } catch (e) { return "E" } }
        property var bN: { root.bump("lv lm n"); try { return n } catch (e) { return "E" } }
        property var bMd: { root.bump("lv lm md"); try { return root.s(modelData) + (modelData && modelData.name ? "." + modelData.name : "") } catch (e) { return "E" } }
        property var bIndex: { root.bump("lv lm index"); try { return index + "/" + model.index } catch (e) { return "E" } }
        function st() { return born + ":" + bName + "," + bMName + "," + bN + "," + bMd + "," + bIndex }
    } }
    property int num_lv: 2
    ListView { id: lv_num; width: 100; height: 100; model: root.num_lv; delegate: Item {
        id: d
        width: 10; height: 10
        property int born: root.born("lv num")
        property var bName: { root.bump("lv num name"); try { return name } catch (e) { return "E" } }
        property var bMName: { root.bump("lv num mname"); try { return model.name } catch (e) { return "E" } }
        property var bN: { root.bump("lv num n"); try { return n } catch (e) { return "E" } }
        property var bMd: { root.bump("lv num md"); try { return root.s(modelData) + (modelData && modelData.name ? "." + modelData.name : "") } catch (e) { return "E" } }
        property var bIndex: { root.bump("lv num index"); try { return index + "/" + model.index } catch (e) { return "E" } }
        function st() { return born + ":" + bName + "," + bMName + "," + bN + "," + bMd + "," + bIndex }
    } }
    property var arro_lv: [{ name: "a", n: 1 }, { name: "b", n: 2 }]
    ListView { id: lv_arro; width: 100; height: 100; model: root.arro_lv; delegate: Item {
        id: d
        width: 10; height: 10
        property int born: root.born("lv arro")
        property var bName: { root.bump("lv arro name"); try { return name } catch (e) { return "E" } }
        property var bMName: { root.bump("lv arro mname"); try { return model.name } catch (e) { return "E" } }
        property var bN: { root.bump("lv arro n"); try { return n } catch (e) { return "E" } }
        property var bMd: { root.bump("lv arro md"); try { return root.s(modelData) + (modelData && modelData.name ? "." + modelData.name : "") } catch (e) { return "E" } }
        property var bIndex: { root.bump("lv arro index"); try { return index + "/" + model.index } catch (e) { return "E" } }
        function st() { return born + ":" + bName + "," + bMName + "," + bN + "," + bMd + "," + bIndex }
    } }
    property list<QtObject> objs_lv: [ QtObject { property string name: "a"; property int n: 1 }, QtObject { property string name: "b"; property int n: 2 } ]
    QtObject { id: spare_lv; property string name: "s"; property int n: 7 }
    ListView { id: lv_objs; width: 100; height: 100; model: root.objs_lv; delegate: Item {
        id: d
        width: 10; height: 10
        property int born: root.born("lv objs")
        property var bName: { root.bump("lv objs name"); try { return name } catch (e) { return "E" } }
        property var bMName: { root.bump("lv objs mname"); try { return model.name } catch (e) { return "E" } }
        property var bN: { root.bump("lv objs n"); try { return n } catch (e) { return "E" } }
        property var bMd: { root.bump("lv objs md"); try { return root.s(modelData) + (modelData && modelData.name ? "." + modelData.name : "") } catch (e) { return "E" } }
        property var bIndex: { root.bump("lv objs index"); try { return index + "/" + model.index } catch (e) { return "E" } }
        function st() { return born + ":" + bName + "," + bMName + "," + bN + "," + bMd + "," + bIndex }
    } }
    ListModel { id: lm_gv; ListElement { name: "a"; n: 1 } ListElement { name: "b"; n: 2 } ListElement { name: "c"; n: 3 } }
    GridView { id: gv_lm; width: 100; height: 100; cellWidth: 10; cellHeight: 10; model: lm_gv; delegate: Item {
        id: d
        width: 10; height: 10
        property int born: root.born("gv lm")
        property var bName: { root.bump("gv lm name"); try { return name } catch (e) { return "E" } }
        property var bMName: { root.bump("gv lm mname"); try { return model.name } catch (e) { return "E" } }
        property var bN: { root.bump("gv lm n"); try { return n } catch (e) { return "E" } }
        property var bMd: { root.bump("gv lm md"); try { return root.s(modelData) + (modelData && modelData.name ? "." + modelData.name : "") } catch (e) { return "E" } }
        property var bIndex: { root.bump("gv lm index"); try { return index + "/" + model.index } catch (e) { return "E" } }
        function st() { return born + ":" + bName + "," + bMName + "," + bN + "," + bMd + "," + bIndex }
    } }
    property int num_gv: 2
    GridView { id: gv_num; width: 100; height: 100; cellWidth: 10; cellHeight: 10; model: root.num_gv; delegate: Item {
        id: d
        width: 10; height: 10
        property int born: root.born("gv num")
        property var bName: { root.bump("gv num name"); try { return name } catch (e) { return "E" } }
        property var bMName: { root.bump("gv num mname"); try { return model.name } catch (e) { return "E" } }
        property var bN: { root.bump("gv num n"); try { return n } catch (e) { return "E" } }
        property var bMd: { root.bump("gv num md"); try { return root.s(modelData) + (modelData && modelData.name ? "." + modelData.name : "") } catch (e) { return "E" } }
        property var bIndex: { root.bump("gv num index"); try { return index + "/" + model.index } catch (e) { return "E" } }
        function st() { return born + ":" + bName + "," + bMName + "," + bN + "," + bMd + "," + bIndex }
    } }
    property var arro_gv: [{ name: "a", n: 1 }, { name: "b", n: 2 }]
    GridView { id: gv_arro; width: 100; height: 100; cellWidth: 10; cellHeight: 10; model: root.arro_gv; delegate: Item {
        id: d
        width: 10; height: 10
        property int born: root.born("gv arro")
        property var bName: { root.bump("gv arro name"); try { return name } catch (e) { return "E" } }
        property var bMName: { root.bump("gv arro mname"); try { return model.name } catch (e) { return "E" } }
        property var bN: { root.bump("gv arro n"); try { return n } catch (e) { return "E" } }
        property var bMd: { root.bump("gv arro md"); try { return root.s(modelData) + (modelData && modelData.name ? "." + modelData.name : "") } catch (e) { return "E" } }
        property var bIndex: { root.bump("gv arro index"); try { return index + "/" + model.index } catch (e) { return "E" } }
        function st() { return born + ":" + bName + "," + bMName + "," + bN + "," + bMd + "," + bIndex }
    } }
    property list<QtObject> objs_gv: [ QtObject { property string name: "a"; property int n: 1 }, QtObject { property string name: "b"; property int n: 2 } ]
    QtObject { id: spare_gv; property string name: "s"; property int n: 7 }
    GridView { id: gv_objs; width: 100; height: 100; cellWidth: 10; cellHeight: 10; model: root.objs_gv; delegate: Item {
        id: d
        width: 10; height: 10
        property int born: root.born("gv objs")
        property var bName: { root.bump("gv objs name"); try { return name } catch (e) { return "E" } }
        property var bMName: { root.bump("gv objs mname"); try { return model.name } catch (e) { return "E" } }
        property var bN: { root.bump("gv objs n"); try { return n } catch (e) { return "E" } }
        property var bMd: { root.bump("gv objs md"); try { return root.s(modelData) + (modelData && modelData.name ? "." + modelData.name : "") } catch (e) { return "E" } }
        property var bIndex: { root.bump("gv objs index"); try { return index + "/" + model.index } catch (e) { return "E" } }
        function st() { return born + ":" + bName + "," + bMName + "," + bN + "," + bMd + "," + bIndex }
    } }
    ListModel { id: lm_inst; ListElement { name: "a"; n: 1 } ListElement { name: "b"; n: 2 } ListElement { name: "c"; n: 3 } }
    Instantiator { id: inst_lm; model: lm_inst; delegate: Item {
        id: d
        width: 10; height: 10
        property int born: root.born("inst lm")
        property var bName: { root.bump("inst lm name"); try { return name } catch (e) { return "E" } }
        property var bMName: { root.bump("inst lm mname"); try { return model.name } catch (e) { return "E" } }
        property var bN: { root.bump("inst lm n"); try { return n } catch (e) { return "E" } }
        property var bMd: { root.bump("inst lm md"); try { return root.s(modelData) + (modelData && modelData.name ? "." + modelData.name : "") } catch (e) { return "E" } }
        property var bIndex: { root.bump("inst lm index"); try { return index + "/" + model.index } catch (e) { return "E" } }
        function st() { return born + ":" + bName + "," + bMName + "," + bN + "," + bMd + "," + bIndex }
    } }
    property int num_inst: 2
    Instantiator { id: inst_num; model: root.num_inst; delegate: Item {
        id: d
        width: 10; height: 10
        property int born: root.born("inst num")
        property var bName: { root.bump("inst num name"); try { return name } catch (e) { return "E" } }
        property var bMName: { root.bump("inst num mname"); try { return model.name } catch (e) { return "E" } }
        property var bN: { root.bump("inst num n"); try { return n } catch (e) { return "E" } }
        property var bMd: { root.bump("inst num md"); try { return root.s(modelData) + (modelData && modelData.name ? "." + modelData.name : "") } catch (e) { return "E" } }
        property var bIndex: { root.bump("inst num index"); try { return index + "/" + model.index } catch (e) { return "E" } }
        function st() { return born + ":" + bName + "," + bMName + "," + bN + "," + bMd + "," + bIndex }
    } }
    property var arro_inst: [{ name: "a", n: 1 }, { name: "b", n: 2 }]
    Instantiator { id: inst_arro; model: root.arro_inst; delegate: Item {
        id: d
        width: 10; height: 10
        property int born: root.born("inst arro")
        property var bName: { root.bump("inst arro name"); try { return name } catch (e) { return "E" } }
        property var bMName: { root.bump("inst arro mname"); try { return model.name } catch (e) { return "E" } }
        property var bN: { root.bump("inst arro n"); try { return n } catch (e) { return "E" } }
        property var bMd: { root.bump("inst arro md"); try { return root.s(modelData) + (modelData && modelData.name ? "." + modelData.name : "") } catch (e) { return "E" } }
        property var bIndex: { root.bump("inst arro index"); try { return index + "/" + model.index } catch (e) { return "E" } }
        function st() { return born + ":" + bName + "," + bMName + "," + bN + "," + bMd + "," + bIndex }
    } }
    property list<QtObject> objs_inst: [ QtObject { property string name: "a"; property int n: 1 }, QtObject { property string name: "b"; property int n: 2 } ]
    QtObject { id: spare_inst; property string name: "s"; property int n: 7 }
    Instantiator { id: inst_objs; model: root.objs_inst; delegate: Item {
        id: d
        width: 10; height: 10
        property int born: root.born("inst objs")
        property var bName: { root.bump("inst objs name"); try { return name } catch (e) { return "E" } }
        property var bMName: { root.bump("inst objs mname"); try { return model.name } catch (e) { return "E" } }
        property var bN: { root.bump("inst objs n"); try { return n } catch (e) { return "E" } }
        property var bMd: { root.bump("inst objs md"); try { return root.s(modelData) + (modelData && modelData.name ? "." + modelData.name : "") } catch (e) { return "E" } }
        property var bIndex: { root.bump("inst objs index"); try { return index + "/" + model.index } catch (e) { return "E" } }
        function st() { return born + ":" + bName + "," + bMName + "," + bN + "," + bMd + "," + bIndex }
    } }
    function run() {
        dump("rep lm", rep_lm, "itemAt", "start")
        step("rep lm", rep_lm, "itemAt", "setProperty", () => lm_rep.setProperty(1, "name", "B"))
        step("rep lm", rep_lm, "itemAt", "set two", () => lm_rep.set(1, { name: "bb", n: 22 }))
        step("rep lm", rep_lm, "itemAt", "set same", () => lm_rep.set(1, { name: "bb", n: 22 }))
        step("rep lm", rep_lm, "itemAt", "set new role", () => lm_rep.set(1, { extra: 5 }))
        step("rep lm", rep_lm, "itemAt", "get().name=", () => { lm_rep.get(2).name = "C" })
        step("rep lm", rep_lm, "itemAt", "append", () => lm_rep.append({ name: "d", n: 4 }))
        step("rep lm", rep_lm, "itemAt", "insert 0", () => lm_rep.insert(0, { name: "z", n: 0 }))
        step("rep lm", rep_lm, "itemAt", "move 0->2", () => lm_rep.move(0, 2, 1))
        step("rep lm", rep_lm, "itemAt", "remove 1", () => lm_rep.remove(1))
        step("rep lm", rep_lm, "itemAt", "set past end", () => lm_rep.set(lm_rep.count, { name: "e", n: 5 }))
        step("rep lm", rep_lm, "itemAt", "clear", () => lm_rep.clear())
        step("rep lm", rep_lm, "itemAt", "append after clear", () => lm_rep.append({ name: "f", n: 6 }))
        dump("rep num", rep_num, "itemAt", "start")
        step("rep num", rep_num, "itemAt", "3", () => { root.num_rep = 3 })
        step("rep num", rep_num, "itemAt", "1", () => { root.num_rep = 1 })
        step("rep num", rep_num, "itemAt", "same", () => { root.num_rep = 1 })
        dump("rep arro", rep_arro, "itemAt", "start")
        step("rep arro", rep_arro, "itemAt", "element.name=", () => { root.arro_rep[1].name = "B" })
        step("rep arro", rep_arro, "itemAt", "push", () => { root.arro_rep.push({ name: "c", n: 3 }) })
        step("rep arro", rep_arro, "itemAt", "reassign same content", () => { root.arro_rep = [{ name: "a", n: 1 }, { name: "B", n: 2 }] })
        step("rep arro", rep_arro, "itemAt", "reassign longer", () => { root.arro_rep = [{ name: "a", n: 1 }, { name: "x", n: 2 }, { name: "y", n: 3 }] })
        step("rep arro", rep_arro, "itemAt", "reassign shorter", () => { root.arro_rep = [{ name: "q", n: 9 }] })
        dump("rep objs", rep_objs, "itemAt", "start")
        step("rep objs", rep_objs, "itemAt", "object.name=", () => { root.objs_rep[1].name = "B" })
        step("rep objs", rep_objs, "itemAt", "object.n=", () => { root.objs_rep[1].n = 22 })
        step("rep objs", rep_objs, "itemAt", "push", () => { root.objs_rep.push(spare_rep) })
        step("rep objs", rep_objs, "itemAt", "reassign", () => { root.objs_rep = [spare_rep] })
        dump("lv lm", lv_lm, "itemAtIndex", "start")
        step("lv lm", lv_lm, "itemAtIndex", "setProperty", () => lm_lv.setProperty(1, "name", "B"))
        step("lv lm", lv_lm, "itemAtIndex", "set two", () => lm_lv.set(1, { name: "bb", n: 22 }))
        step("lv lm", lv_lm, "itemAtIndex", "set same", () => lm_lv.set(1, { name: "bb", n: 22 }))
        step("lv lm", lv_lm, "itemAtIndex", "set new role", () => lm_lv.set(1, { extra: 5 }))
        step("lv lm", lv_lm, "itemAtIndex", "get().name=", () => { lm_lv.get(2).name = "C" })
        step("lv lm", lv_lm, "itemAtIndex", "append", () => lm_lv.append({ name: "d", n: 4 }))
        step("lv lm", lv_lm, "itemAtIndex", "insert 0", () => lm_lv.insert(0, { name: "z", n: 0 }))
        step("lv lm", lv_lm, "itemAtIndex", "move 0->2", () => lm_lv.move(0, 2, 1))
        step("lv lm", lv_lm, "itemAtIndex", "remove 1", () => lm_lv.remove(1))
        step("lv lm", lv_lm, "itemAtIndex", "set past end", () => lm_lv.set(lm_lv.count, { name: "e", n: 5 }))
        step("lv lm", lv_lm, "itemAtIndex", "clear", () => lm_lv.clear())
        step("lv lm", lv_lm, "itemAtIndex", "append after clear", () => lm_lv.append({ name: "f", n: 6 }))
        dump("lv num", lv_num, "itemAtIndex", "start")
        step("lv num", lv_num, "itemAtIndex", "3", () => { root.num_lv = 3 })
        step("lv num", lv_num, "itemAtIndex", "1", () => { root.num_lv = 1 })
        step("lv num", lv_num, "itemAtIndex", "same", () => { root.num_lv = 1 })
        dump("lv arro", lv_arro, "itemAtIndex", "start")
        step("lv arro", lv_arro, "itemAtIndex", "element.name=", () => { root.arro_lv[1].name = "B" })
        step("lv arro", lv_arro, "itemAtIndex", "reassign same content", () => { root.arro_lv = [{ name: "a", n: 1 }, { name: "B", n: 2 }] })
        step("lv arro", lv_arro, "itemAtIndex", "reassign longer", () => { root.arro_lv = [{ name: "a", n: 1 }, { name: "x", n: 2 }, { name: "y", n: 3 }] })
        step("lv arro", lv_arro, "itemAtIndex", "reassign shorter", () => { root.arro_lv = [{ name: "q", n: 9 }] })
        dump("lv objs", lv_objs, "itemAtIndex", "start")
        step("lv objs", lv_objs, "itemAtIndex", "object.name=", () => { root.objs_lv[1].name = "B" })
        step("lv objs", lv_objs, "itemAtIndex", "object.n=", () => { root.objs_lv[1].n = 22 })
        step("lv objs", lv_objs, "itemAtIndex", "push", () => { root.objs_lv.push(spare_lv) })
        step("lv objs", lv_objs, "itemAtIndex", "reassign", () => { root.objs_lv = [spare_lv] })
        dump("gv lm", gv_lm, "itemAtIndex", "start")
        step("gv lm", gv_lm, "itemAtIndex", "setProperty", () => lm_gv.setProperty(1, "name", "B"))
        step("gv lm", gv_lm, "itemAtIndex", "set two", () => lm_gv.set(1, { name: "bb", n: 22 }))
        step("gv lm", gv_lm, "itemAtIndex", "set same", () => lm_gv.set(1, { name: "bb", n: 22 }))
        step("gv lm", gv_lm, "itemAtIndex", "set new role", () => lm_gv.set(1, { extra: 5 }))
        step("gv lm", gv_lm, "itemAtIndex", "get().name=", () => { lm_gv.get(2).name = "C" })
        step("gv lm", gv_lm, "itemAtIndex", "append", () => lm_gv.append({ name: "d", n: 4 }))
        step("gv lm", gv_lm, "itemAtIndex", "insert 0", () => lm_gv.insert(0, { name: "z", n: 0 }))
        step("gv lm", gv_lm, "itemAtIndex", "move 0->2", () => lm_gv.move(0, 2, 1))
        step("gv lm", gv_lm, "itemAtIndex", "remove 1", () => lm_gv.remove(1))
        step("gv lm", gv_lm, "itemAtIndex", "set past end", () => lm_gv.set(lm_gv.count, { name: "e", n: 5 }))
        step("gv lm", gv_lm, "itemAtIndex", "clear", () => lm_gv.clear())
        step("gv lm", gv_lm, "itemAtIndex", "append after clear", () => lm_gv.append({ name: "f", n: 6 }))
        dump("gv num", gv_num, "itemAtIndex", "start")
        step("gv num", gv_num, "itemAtIndex", "3", () => { root.num_gv = 3 })
        step("gv num", gv_num, "itemAtIndex", "1", () => { root.num_gv = 1 })
        step("gv num", gv_num, "itemAtIndex", "same", () => { root.num_gv = 1 })
        dump("gv arro", gv_arro, "itemAtIndex", "start")
        step("gv arro", gv_arro, "itemAtIndex", "element.name=", () => { root.arro_gv[1].name = "B" })
        step("gv arro", gv_arro, "itemAtIndex", "reassign same content", () => { root.arro_gv = [{ name: "a", n: 1 }, { name: "B", n: 2 }] })
        step("gv arro", gv_arro, "itemAtIndex", "reassign longer", () => { root.arro_gv = [{ name: "a", n: 1 }, { name: "x", n: 2 }, { name: "y", n: 3 }] })
        step("gv arro", gv_arro, "itemAtIndex", "reassign shorter", () => { root.arro_gv = [{ name: "q", n: 9 }] })
        dump("gv objs", gv_objs, "itemAtIndex", "start")
        step("gv objs", gv_objs, "itemAtIndex", "object.name=", () => { root.objs_gv[1].name = "B" })
        step("gv objs", gv_objs, "itemAtIndex", "object.n=", () => { root.objs_gv[1].n = 22 })
        step("gv objs", gv_objs, "itemAtIndex", "push", () => { root.objs_gv.push(spare_gv) })
        step("gv objs", gv_objs, "itemAtIndex", "reassign", () => { root.objs_gv = [spare_gv] })
        dump("inst lm", inst_lm, "objectAt", "start")
        step("inst lm", inst_lm, "objectAt", "setProperty", () => lm_inst.setProperty(1, "name", "B"))
        step("inst lm", inst_lm, "objectAt", "set two", () => lm_inst.set(1, { name: "bb", n: 22 }))
        step("inst lm", inst_lm, "objectAt", "set same", () => lm_inst.set(1, { name: "bb", n: 22 }))
        step("inst lm", inst_lm, "objectAt", "set new role", () => lm_inst.set(1, { extra: 5 }))
        step("inst lm", inst_lm, "objectAt", "get().name=", () => { lm_inst.get(2).name = "C" })
        dump("inst num", inst_num, "objectAt", "start")
        step("inst num", inst_num, "objectAt", "3", () => { root.num_inst = 3 })
        step("inst num", inst_num, "objectAt", "1", () => { root.num_inst = 1 })
        step("inst num", inst_num, "objectAt", "same", () => { root.num_inst = 1 })
        dump("inst arro", inst_arro, "objectAt", "start")
        step("inst arro", inst_arro, "objectAt", "element.name=", () => { root.arro_inst[1].name = "B" })
        step("inst arro", inst_arro, "objectAt", "push", () => { root.arro_inst.push({ name: "c", n: 3 }) })
        step("inst arro", inst_arro, "objectAt", "reassign same content", () => { root.arro_inst = [{ name: "a", n: 1 }, { name: "B", n: 2 }] })
        step("inst arro", inst_arro, "objectAt", "reassign longer", () => { root.arro_inst = [{ name: "a", n: 1 }, { name: "x", n: 2 }, { name: "y", n: 3 }] })
        step("inst arro", inst_arro, "objectAt", "reassign shorter", () => { root.arro_inst = [{ name: "q", n: 9 }] })
        dump("inst objs", inst_objs, "objectAt", "start")
        step("inst objs", inst_objs, "objectAt", "object.name=", () => { root.objs_inst[1].name = "B" })
        step("inst objs", inst_objs, "objectAt", "object.n=", () => { root.objs_inst[1].n = 22 })
        step("inst objs", inst_objs, "objectAt", "push", () => { root.objs_inst.push(spare_inst) })
        step("inst objs", inst_objs, "objectAt", "reassign", () => { root.objs_inst = [spare_inst] })
        result = seen.join(";")
    }
    Component.onCompleted: Qt.callLater(run)
}
