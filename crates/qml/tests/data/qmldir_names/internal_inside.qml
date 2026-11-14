import QtQuick
import "q"
Item {
    id: root
    function t(f) { try { var v = f(); return typeof v === "string" ? v : typeof v + ":" + v } catch (e) { return "!" + e } }
    UseHid { id: u }
    property string result: u.r + " " + u.s + " " + String(Hid) + " " + String(UseHid)
}
