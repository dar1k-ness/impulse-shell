// A property declaration may end with `;` whatever its value is: an object, a list, a block.
// qt: 7 8 1 9 1 2
import QtQuick
Item {
    id: root
    property Item sub: Item { width: 7 } ;
    readonly property Item ro: Item { width: 8 } ;
    property list<Item> l: [Item {}] ;
    property var v: { 9 } ;
    property int a: 1;
    property int two: 2;;
    property string result: [sub.width, ro.width, l.length, v, a, two].join(" ")
}
