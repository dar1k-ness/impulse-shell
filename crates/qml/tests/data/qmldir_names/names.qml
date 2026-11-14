import QtQuick
import "q"
Item {
    id: root
    function t(f) { try { var v = f(); return typeof v === "string" ? v : typeof v + ":" + v } catch (e) { return "!" + e } }

    property string result: [() => typeof Renamed, () => Renamed.v, () => typeof Uno, () => Uno.v, () => typeof Other, () => String(Other), () => typeof Plain, () => String(Plain), () => typeof Free, () => String(Free), () => typeof Hidden, () => typeof Hid].map(f => t(f)).join(" | ")
}
