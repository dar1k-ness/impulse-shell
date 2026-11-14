// `name: ;` is an empty statement (UiScriptStatement: EmptyStatement), whose value is
// undefined: a `var` property is undefined, a typed one keeps its default, a handler does
// nothing.
// qt: undefined undefined undefined undefined 0 7 2
import QtQuick
Item {
    id: root
    property var c: ;
    property var d: ;;
    readonly property var e: ;
    property var g:
        ;
    property int n: ;
    height: 7
    property int hits: 0
    onHeightChanged: ;
    onNChanged: hits += 1
    property var after: ; property int last: 2
    property string result: [typeof c, typeof d, typeof e, typeof g, n, height, last].join(" ")
}
