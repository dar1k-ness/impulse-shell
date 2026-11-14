import QtQuick
import Qt.labs.synchronizer
Item {
    id: root
    property int a: 1
    property int twice: a * 2
    property int b: 3
    Item { id: other; property int c: 10
        property int cc: c + 100
        onCChanged: console.log("cChanged", c, "a", root.a, "cc", cc)
        onCcChanged: console.log("ccChanged", cc, "a", root.a)
        Synchronizer { id: sy; sourceObject: root; sourceProperty: "a"; targetObject: other; targetProperty: "c"
            onValueBounced: (o, p) => console.log("bounced", o === root ? "root" : o === other ? "other" : o, p)
            onValueIgnored: (o, p) => console.log("ignored", o === root ? "root" : o === other ? "other" : o, p) }
    }
    onAChanged: console.log("aChanged", a, "c", other.c, "twice", twice)
    onTwiceChanged: console.log("twiceChanged", twice, "c", other.c)
    Component.onCompleted: {
        console.log("start a", a, "c", other.c);
        a = 5; console.log("a=5 ->", a, other.c);
        other.c = 7; console.log("c=7 ->", a, other.c);
        sy.sourceProperty = "b"; console.log("source b ->", a, b, other.c);
        b = 4; console.log("b=4 ->", a, b, other.c);
        a = 6; console.log("a=6 ->", a, b, other.c);
        sy.targetObject = null; b = 9; console.log("no target ->", a, b, other.c);
        console.log("END"); Qt.quit();
    }
}
