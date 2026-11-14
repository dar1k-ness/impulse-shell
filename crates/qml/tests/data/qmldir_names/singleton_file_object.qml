import QtQuick
import "q"
Item {
    id: root
    function t(f) { try { var v = f(); return typeof v === "string" ? v : typeof v + ":" + v } catch (e) { return "!" + e } }
    Uno { id: o }
    property string result: [() => o.v].map(f => t(f)).join(" | ")
}
