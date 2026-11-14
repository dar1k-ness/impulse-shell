import QtQuick
import Qt.labs.synchronizer
Item {
    id: root
    property int a: 1
    property real r: 2.5
    property string s: "x"
    property var v
    property int clamped: 0
    onClampedChanged: if (clamped > 10) clamped = 10
    property int ro: 3
    property var ignored: []
    Item { id: other; property int c: 10 }
    Synchronizer { id: sy
        sourceObject: root; sourceProperty: "a"
        targetObject: other; targetProperty: "c"
        property alias third: root.r
        property alias fourth: root.s
        property alias fifth: root.clamped
        onValueBounced: (o, p) => console.log("bounced", o === root, p)
        // Qt reports the ends of one change in the order of a QHash: sorted in dump().
        onValueIgnored: (o, p) => root.ignored.push((o === sy ? "sy." : o === other ? "other." : "?.") + p)
    }
    function dump(t) {
        console.log(t, a, other.c, r, JSON.stringify(s), clamped, "ignored", ignored.sort().join(" "))
        ignored = []
    }
    Component.onCompleted: {
        dump("start");
        a = 5; dump("a=5");
        other.c = 20; dump("c=20");
        r = 3.7; dump("r=3.7");
        s = "42"; dump("s=42");
        s = "zz"; dump("s=zz");
        a = 5; dump("a=5 again");
        sy.targetProperty = "nope"; dump("bad target");
        a = 6; dump("a=6");
        sy.targetObject = null; sy.targetProperty = "c"; a = 7; dump("no target a=7");
        sy.targetObject = other; dump("target back");
        a = 8; dump("a=8");
        sy.sourceObject = null; a = 9; dump("no source a=9");
        other.c = 1; dump("c=1");
        console.log(sy.sourceObject, sy.sourceProperty, sy.targetObject === other, sy.targetProperty, typeof sy.valueBounced, typeof sy.valueIgnored);
        console.log("END"); Qt.quit()
    }
}
