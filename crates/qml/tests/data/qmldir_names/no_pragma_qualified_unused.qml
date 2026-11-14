import QtQuick
import "n" as N
Item {
    id: root
    function t(f) { try { var v = f(); return typeof v === "string" ? v : typeof v + ":" + v } catch (e) { return "!" + e } }
    property string result: "unused-qualified"
}
