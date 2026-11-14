import QtQuick
import "q" as QQ
Item {
    id: root
    function t(f) { try { var v = f(); return typeof v === "string" ? v : typeof v + ":" + v } catch (e) { return "!" + e } }
    QQ.Other { id: o }  QQ.Free { id: o2 }
    property string result: [() => o.p + o2.fr].map(f => t(f)).join(" | ")
}
