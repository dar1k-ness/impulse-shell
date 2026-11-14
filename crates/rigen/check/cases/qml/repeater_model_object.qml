// A Repeater delegate has `model` (with `index` and `modelData`) over a number, a string or
// an array as well; a number is rounded, a string of digits is a count and any other string
// is its characters; a ListModel's roles are names of the delegate and members of `model`.
// qt: n=2:0/0/0/object,1/1/1/object | f=3:0 | neg=0 | s=3:a/a,b/b,c/c | d=12 | b=1:0/0 | arr=2:x/x/0,7/7/1 | obj=1:undefined/1/0 | lm=2:one/1/one/0/0,two/2/two/1/1 | one=1:solo/solo/solo | req=1:1/0 | own=1:5 | set=2:ONE/1/ONE/0/0,two/2/two/1/1 | ins=3:zero/0/zero/0/0,ONE/1/ONE/1/1,two/2/two/2/2
import QtQuick
Item {
    id: root
    property string result: ""
    ListModel { id: lm; ListElement { name: "one"; val: 1 } ListElement { name: "two"; val: 2 } }
    ListModel { id: lm1; ListElement { name: "solo" } }
    Repeater { id: rn; model: 2; Item { property string s: index + "/" + modelData + "/" + model.index + "/" + typeof model } }
    Repeater { id: rf; model: 2.5; Item { property string s: "" + model.modelData } }
    Repeater { id: rneg; model: -3; Item { property string s: "x" } }
    Repeater { id: rs; model: "abc"; Item { property string s: modelData + "/" + model.modelData } }
    Repeater { id: rd; model: "12"; Item { property string s: "" + index } }
    Repeater { id: rb; model: true; Item { property string s: modelData + "/" + model.index } }
    Repeater { id: ra; model: ["x", 7]; Item { property string s: modelData + "/" + model.modelData + "/" + model.index } }
    Repeater { id: ro; model: [{ day: 1 }]; Item { property string s: model.day + "/" + modelData.day + "/" + model.index } }
    Repeater { id: rl; model: lm; Item { property string s: name + "/" + val + "/" + model.name + "/" + index + "/" + model.index } }
    Repeater { id: r1; model: lm1; Item { property string s: name + "/" + model.name + "/" + model.modelData } }
    Repeater { id: rq; model: [{ day: 1 }]; Item { required property var model; property string s: model.day + "/" + model.index } }
    Repeater { id: rown; model: 1; Item { property int model: 5; property string s: "" + model } }
    function rows(r) {
        var out = []
        for (var i = 0; i < r.count; i++)
            out.push(r.itemAt(i).s)
        return r.count + ":" + out
    }
    Component.onCompleted: {
        var out = ["n=" + rows(rn), "f=" + rows(rf).slice(0, 3), "neg=" + rneg.count, "s=" + rows(rs), "d=" + rd.count,
            "b=" + rows(rb), "arr=" + rows(ra), "obj=" + rows(ro), "lm=" + rows(rl), "one=" + rows(r1), "req=" + rows(rq),
            "own=" + rows(rown)]
        lm.setProperty(0, "name", "ONE")
        out.push("set=" + rows(rl))
        lm.insert(0, { name: "zero", val: 0 })
        out.push("ins=" + rows(rl))
        result = out.join(" | ")
    }
}
