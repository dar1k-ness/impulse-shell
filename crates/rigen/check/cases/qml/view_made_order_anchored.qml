// qt: ftr z | ftr parent | hdr z | hdr parent | d0 z | d0 parent | d0 done | sec_a z | sec_a parent | sec_a done | d1 z | d1 parent | d1 done | d2 z | d2 parent | d2 done | sec_b z | sec_b parent | sec_b done | hl parent | hl done || ftr,hdr,sec_a,d0,d1,sec_b,d2,hl
import QtQuick
Item {
    id: root
    width: 200; height: 200
    property var log: []
    property string result: ""
    function kids(v) { var o = []; for (var i = 0; i < v.contentItem.children.length; i++) o.push(v.contentItem.children[i].objectName || "?"); return o.join(",") }
    ListModel { id: lm; ListElement { g: "a" } ListElement { g: "a" } ListElement { g: "b" } }
    ListView {
        id: v
        anchors.fill: parent
        model: lm
        header: Item { objectName: "hdr"; height: 5; onZChanged: root.log.push("hdr z"); onParentChanged: root.log.push("hdr parent") }
        footer: Item { objectName: "ftr"; height: 5; onZChanged: root.log.push("ftr z"); onParentChanged: root.log.push("ftr parent") }
        highlight: Item { objectName: "hl"; onParentChanged: root.log.push("hl parent"); Component.onCompleted: root.log.push("hl done") }
        section.property: "g"
        section.delegate: Item { objectName: "sec_" + section; height: 3; onZChanged: root.log.push("sec_" + section + " z"); onParentChanged: root.log.push("sec_" + section + " parent"); Component.onCompleted: root.log.push(objectName + " done") }
        delegate: Item {
            objectName: "d" + index
            height: 20; width: 50
            onZChanged: root.log.push("d" + index + " z")
            onParentChanged: root.log.push("d" + index + " parent")
            Component.onCompleted: root.log.push("d" + index + " done")
        }
    }
    Component.onCompleted: result = log.join(" | ") + " || " + kids(v)
}
