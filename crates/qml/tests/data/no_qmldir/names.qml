import QtQuick
import "d"
import "e" as E
QtObject {
    function tt(f) { try { var v = f(); return typeof v + ":" + v } catch (e) { return "!" + e } }
    property Plain p: Plain {}
    property var cases: [
        () => Solo.v,
        () => typeof Solo,
        () => E.Solo2.v,
        () => typeof E.Solo2,
        () => p.v,
        () => Solo === Solo
    ]
    property string result: cases.map(f => tt(f)).join(" | ")
}
