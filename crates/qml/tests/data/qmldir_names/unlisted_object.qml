import QtQuick
import "q"
Item {
    id: root
    function t(f) { try { var v = f(); return typeof v === "string" ? v : typeof v + ":" + v } catch (e) { return "!" + e } }
    Free { id: o }
    property string result: [() => o.fr].map(f => t(f)).join(" | ")
}
