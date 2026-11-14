import QtQuick
import "n"
Item {
    id: root
    function t(f) { try { var v = f(); return typeof v === "string" ? v : typeof v + ":" + v } catch (e) { return "!" + e } }

    property string result: [() => typeof Np, () => Np.v].map(f => t(f)).join(" | ")
}
