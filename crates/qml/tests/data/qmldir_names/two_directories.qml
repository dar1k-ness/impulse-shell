import QtQuick
import "q"
import "s"
Item {
    id: root
    function t(f) { try { var v = f(); return typeof v === "string" ? v : typeof v + ":" + v } catch (e) { return "!" + e } }

    property string result: [() => Renamed.v + Single.v].map(f => t(f)).join(" | ")
}
