import QtQuick
import "s" as S
Item {
    id: root
    function t(f) { try { var v = f(); return typeof v === "string" ? v : typeof v + ":" + v } catch (e) { return "!" + e } }

    property string result: [() => S.Single.v, () => typeof Single, () => S.Single === S.Single, () => typeof S.Single].map(f => t(f)).join(" | ")
}
