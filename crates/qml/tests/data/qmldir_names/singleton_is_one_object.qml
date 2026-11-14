import QtQuick
import "q"
Item {
    id: root
    function t(f) { try { var v = f(); return typeof v === "string" ? v : typeof v + ":" + v } catch (e) { return "!" + e } }
    property var a: Renamed
    property var b: Uno
    property string result
    Component.onCompleted: result = (a === b) + " " + (a === Renamed) + " " + a.v + " " + (Renamed.v = 11) + " " + Uno.v + " " + typeof Renamed.vChanged
}
