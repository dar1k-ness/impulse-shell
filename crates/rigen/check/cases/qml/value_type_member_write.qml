// A write to a member of a value type property (`p.x = 10`) sends the property's change
// signal once.
// qt: 1,10;1,5;1,11;1,9;1,7;
import QtQuick
Item {
    id: root
    property string result: ""
    property point p: Qt.point(1, 2)
    property int np: 0
    onPChanged: np++
    property rect r: Qt.rect(1, 2, 3, 4)
    property int nr: 0
    onRChanged: nr++
    Component.onCompleted: {
        var n = np; p.x = 10; result += (np - n) + "," + p.x + ";"
        n = np; root.p.y = 5; result += (np - n) + "," + p.y + ";"
        n = np; p.x += 1; result += (np - n) + "," + p.x + ";"
        n = nr; r.width = 9; result += (nr - n) + "," + r.width + ";"
        n = nr; [r.height] = [7]; result += (nr - n) + "," + r.height + ";"
    }
}
