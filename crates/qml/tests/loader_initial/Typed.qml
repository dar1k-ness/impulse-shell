import QtQuick
Item {
    property url u
    property string s
    property int i
    property real r
    property bool b
    property color c
    property point p
    property date d
    property font f
    property var v
    property QtObject o
    property Item it
    property list<int> li
    property rect rc
    property size sz
    readonly property int ro: 7
    property int bound: i + 1
    property alias al: inner.width
    Item { id: inner }
    width: 5
}
