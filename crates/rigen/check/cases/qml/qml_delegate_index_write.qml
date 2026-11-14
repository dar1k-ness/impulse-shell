// `index` and `modelData` in a delegate (Qt 6.12, QQmlDelegateModelItem): `index` is
// read-only in the delegate's context, a write to `modelData` goes where Qt puts it, and a
// `required property int index` is the delegate's own property.
// qt: lm wIndex TypeError: Cannot assign to read-only property "index";lm wIndexCompound TypeError: Cannot assign to read-only property "index";lm wIndexIncr TypeError: Cannot assign to read-only property "index";lm wIndexStrict TypeError: Cannot assign to read-only property "index";lm wOwn 2;lm rIndex 1;int wIndex TypeError: Cannot assign to read-only property "index";int wModelData 7;int rIndex 1;arr wIndex TypeError: Cannot assign to read-only property "index";arr wModelData q,y;obj wIndex TypeError: Cannot assign to read-only property "index";obj wModelData string;req wIndex 5;req outside 6;lv wIndex TypeError: Cannot assign to read-only property "index";
import QtQuick
Item {
    id: root
    property string result: ""
    function t(what, f) {
        var r
        try { r = String(f()) } catch (e) { r = e.name + ": " + e.message }
        result += what + " " + r + ";"
    }
    ListModel { id: lm; ListElement { name: "a"; n: 1 } ListElement { name: "b"; n: 2 } }
    Repeater { id: repLm; model: lm
        Item {
            property int own: 0
            function wIndex() { index = 5; return index }
            function wIndexCompound() { index += 5; return index }
            function wIndexIncr() { index++; return index }
            function wIndexStrict() { "use strict"; index = 5; return index }
            function wOwn() { own = 2; return own }
            function rIndex() { return index }
        }
    }
    Repeater { id: repInt; model: 3
        Item {
            function wIndex() { index = 5; return index }
            function wModelData() { modelData = 7; return modelData }
            function rIndex() { return index }
        }
    }
    Repeater { id: repArr; model: ["x", "y"]
        Item {
            function wIndex() { index = 5; return index }
            function wModelData() { modelData = "q"; return modelData + "," + repArr.model[index] }
        }
    }
    Repeater { id: repObj; model: [{ k: 1 }, { k: 2 }]
        Item {
            function wIndex() { index = 5; return index }
            function wModelData() { modelData = "q"; return typeof modelData }
        }
    }
    Repeater { id: repReq; model: lm
        Item {
            required property int index
            required property string name
            function wIndex() { index = 5; return index }
        }
    }
    ListView { id: lv; width: 100; height: 100; model: lm
        delegate: Item { height: 10
            function wIndex() { index = 5; return index }
        }
    }
    Component.onCompleted: Qt.callLater(run)
    function run() {
        var d = repLm.itemAt(1)
        for (var k of ["wIndex", "wIndexCompound", "wIndexIncr", "wIndexStrict", "wOwn", "rIndex"]) t("lm " + k, d[k])
        d = repInt.itemAt(1)
        for (k of ["wIndex", "wModelData", "rIndex"]) t("int " + k, d[k])
        d = repArr.itemAt(1)
        for (k of ["wIndex", "wModelData"]) t("arr " + k, d[k])
        d = repObj.itemAt(1)
        for (k of ["wIndex", "wModelData"]) t("obj " + k, d[k])
        d = repReq.itemAt(1)
        t("req wIndex", d.wIndex)
        t("req outside", () => { d.index = 6; return d.index })
        d = lv.itemAtIndex(1)
        t("lv wIndex", d.wIndex)
    }
}
