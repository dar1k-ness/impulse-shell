import QtQuick
import Qt.labs.synchronizer
Item {
    id: root
    property int a: 1
    property string str: "5"
    Item { id: other; property int c: 10; property int d: 20
        onCChanged: console.log("cChanged", c)
        Synchronizer on c { property alias source: root.a }
        Synchronizer on d { property alias source: root.str }
    }
    onAChanged: console.log("aChanged", a)
    onStrChanged: console.log("strChanged", str)
    Component.onCompleted: {
        console.log("start a", a, "c", other.c, "str", str, "d", other.d);
        a = 5; console.log("a=5 ->", a, other.c);
        other.c = 7; console.log("c=7 ->", a, other.c);
        a = a + 1; a = a + 1; console.log("a+=2 ->", a, other.c);
        other.d = 8; console.log("d=8 ->", JSON.stringify(str), other.d);
        str = "abc"; console.log("str=abc ->", JSON.stringify(str), other.d);
        console.log("END"); Qt.quit();
    }
}
