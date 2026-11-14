import QtQuick
import Qt.labs.synchronizer
Item {
    id: root
    property int a: 1
    property string s: "x"
    property real r: 1.5
    property int clamp: 0
    onClampChanged: if (clamp > 10) clamp = 10
    property int ro: 3
    Item { id: other; property int c: 10; property int d: 0; property int e: 0
        Synchronizer on c { id: s1; property alias source: root.a; property alias extra: root.r
            onValueBounced: (o, p) => console.log("s1 bounced", p)
            onValueIgnored: (o, p) => console.log("s1 ignored", p) }
        Synchronizer on d { id: s2; property alias source: root.clamp
            onValueBounced: (o, p) => console.log("s2 bounced", p)
            onValueIgnored: (o, p) => console.log("s2 ignored", p) }
        Synchronizer on e { id: s3; property alias other: root.s
            onValueBounced: (o, p) => console.log("s3 bounced", p)
            onValueIgnored: (o, p) => console.log("s3 ignored", p) }
    }
    Component.onCompleted: {
        console.log("start", a, r, other.c, "|", clamp, other.d, "|", JSON.stringify(s), other.e);
        a = 4; console.log("a=4", a, r, other.c);
        r = 2.5; console.log("r=2.5", a, r, other.c);
        other.c = 9; console.log("c=9", a, r, other.c);
        other.d = 50; console.log("d=50", clamp, other.d);
        clamp = 3; console.log("clamp=3", clamp, other.d);
        s = "12"; console.log("s=12", JSON.stringify(s), other.e);
        other.e = 8; console.log("e=8", JSON.stringify(s), other.e);
        s = "zz"; console.log("s=zz", JSON.stringify(s), other.e);
        console.log("END"); Qt.quit();
    }
}
