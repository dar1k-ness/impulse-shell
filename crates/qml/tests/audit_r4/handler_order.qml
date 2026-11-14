import QtQuick
Item {
    id: root
    property int n: 0
    property int twice: n * 2
    signal s(int a)
    onS: a => console.log("handler", a, "n", n)
    onNChanged: console.log("nChanged", n, "twice", twice)
    onTwiceChanged: console.log("twiceChanged", twice)
    Component.onCompleted: {
        console.log("before"); s(1); console.log("after emit");
        n = 1; console.log("a twice", twice); n = 2; console.log("b twice", twice);
        console.log("END"); Qt.quit();
    }
}
