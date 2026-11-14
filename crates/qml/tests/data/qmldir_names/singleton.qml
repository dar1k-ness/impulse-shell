import QtQuick
import "s"
Item {
    id: root
    function t(f) { try { var v = f(); return typeof v === "string" ? v : typeof v + ":" + v } catch (e) { return "!" + e } }

    property string result: [() => Single.v, () => typeof Single, () => Single === Single, () => Object.keys(Single).join(",")].map(f => t(f)).join(" | ")
}
