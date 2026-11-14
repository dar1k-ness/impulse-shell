// Who is told of a write in a delegate (Qt 6.12): a write through `modelData` of a ListModel
// of one role tells twice, a role given the number it has tells nobody and one given the
// string it has does; a new name on the `model` of a number's row tells nobody.
// qt: one start 1/1;one modelData=5 3/3 5;one modelData=5 again 4/4;one model.modelData=6 6/6;one num=7 7/7;one setProperty 8/8;ones start 1/1;ones modelData=b 3/3 b;ones modelData=b again 5/5;ones str=c 6/6;ones str=c again 7/7;ones setProperty same 8/8;ones set same 9/9;num start 1/1 E undefined;num model.name=m 1/1 E undefined;num rebound 2/2 m m;num model.name=m2 2/2 m m;num model.name=m2 again 2/2 m m;num name=m3 Error: Invalid write to global property "name" 2/2 m m;num model.name=4 2/2 m m string;num modelData=0 same 1 0;num modelData=3 2 3;num modelData=3 again 2 3
import QtQuick
Item {
    id: root
    width: 400; height: 400
    property string result: ""
    property var seen: []
    property var cnt: ({})
    function bump(k) { cnt[k] = (cnt[k] || 0) + 1 }
    function c(k) { return cnt[k] || 0 }
    ListModel { id: one; ListElement { num: 1 } }
    ListModel { id: ones; ListElement { str: "a" } }
    Repeater { id: r1; model: one; delegate: Item {
        property var a: { root.bump("one num"); return num }
        property var b: { root.bump("one md"); return modelData }
        function go() {
            root.seen.push("one start " + root.c("one num") + "/" + root.c("one md"))
            modelData = 5; root.seen.push("one modelData=5 " + root.c("one num") + "/" + root.c("one md") + " " + num)
            modelData = 5; root.seen.push("one modelData=5 again " + root.c("one num") + "/" + root.c("one md"))
            model.modelData = 6; root.seen.push("one model.modelData=6 " + root.c("one num") + "/" + root.c("one md"))
            num = 7; root.seen.push("one num=7 " + root.c("one num") + "/" + root.c("one md"))
            one.setProperty(0, "num", 8); root.seen.push("one setProperty " + root.c("one num") + "/" + root.c("one md"))
        }
    } }
    Repeater { id: r2; model: ones; delegate: Item {
        property var a: { root.bump("ones str"); return str }
        property var b: { root.bump("ones md"); return modelData }
        function go() {
            root.seen.push("ones start " + root.c("ones str") + "/" + root.c("ones md"))
            modelData = "b"; root.seen.push("ones modelData=b " + root.c("ones str") + "/" + root.c("ones md") + " " + str)
            modelData = "b"; root.seen.push("ones modelData=b again " + root.c("ones str") + "/" + root.c("ones md"))
            str = "c"; root.seen.push("ones str=c " + root.c("ones str") + "/" + root.c("ones md"))
            str = "c"; root.seen.push("ones str=c again " + root.c("ones str") + "/" + root.c("ones md"))
            ones.setProperty(0, "str", "c"); root.seen.push("ones setProperty same " + root.c("ones str") + "/" + root.c("ones md"))
            ones.set(0, { str: "c" }); root.seen.push("ones set same " + root.c("ones str") + "/" + root.c("ones md"))
        }
    } }
    Repeater { id: r3; model: 1; delegate: Item {
        id: d
        property var a: { root.bump("num name"); try { return name } catch (e) { return "E" } }
        property var b: { root.bump("num mname"); return model.name }
        property var k: { root.bump("num md"); return modelData }
        function go() {
            root.seen.push("num start " + root.c("num name") + "/" + root.c("num mname") + " " + a + " " + b)
            model.name = "m"; root.seen.push("num model.name=m " + root.c("num name") + "/" + root.c("num mname") + " " + a + " " + b)
            d.a = Qt.binding(() => { root.bump("num name"); return name })
            d.b = Qt.binding(() => { root.bump("num mname"); return model.name })
            root.seen.push("num rebound " + root.c("num name") + "/" + root.c("num mname") + " " + a + " " + b)
            model.name = "m2"; root.seen.push("num model.name=m2 " + root.c("num name") + "/" + root.c("num mname") + " " + a + " " + b)
            model.name = "m2"; root.seen.push("num model.name=m2 again " + root.c("num name") + "/" + root.c("num mname") + " " + a + " " + b)
            var er = "ok"; try { name = "m3" } catch (e) { er = "" + e }; root.seen.push("num name=m3 " + er + " " + root.c("num name") + "/" + root.c("num mname") + " " + a + " " + b)
            model.name = 4; root.seen.push("num model.name=4 " + root.c("num name") + "/" + root.c("num mname") + " " + a + " " + b + " " + typeof b)
            modelData = 0; root.seen.push("num modelData=0 same " + root.c("num md") + " " + k)
            modelData = 3; root.seen.push("num modelData=3 " + root.c("num md") + " " + k)
            modelData = 3; root.seen.push("num modelData=3 again " + root.c("num md") + " " + k)
        }
    } }
    Component.onCompleted: {
        r1.itemAt(0).go(); r2.itemAt(0).go(); r3.itemAt(0).go()
        result = seen.join(";")
    }
}
