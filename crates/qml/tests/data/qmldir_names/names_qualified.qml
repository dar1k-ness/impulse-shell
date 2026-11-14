import QtQuick
import "q" as QQ
Item {
    id: root
    function t(f) { try { var v = f(); return typeof v === "string" ? v : typeof v + ":" + v } catch (e) { return "!" + e } }

    property string result: [() => QQ.Renamed.v, () => typeof QQ.Uno, () => QQ.Uno.v, () => String(QQ.Other), () => QQ.Renamed === QQ.Renamed, () => String(QQ.Free), () => String(QQ.Hidden), () => String(QQ.Plain)].map(f => t(f)).join(" | ")
}
