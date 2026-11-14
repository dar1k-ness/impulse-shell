// The `qmldir` of a directory imported by its path names its types: `singleton Renamed
// Uno.qml` is one object under both names, `Other 1.0 Plain.qml` a type under both,
// `internal Hidden Hid.qml` a type only inside the directory.
// qt: object 8 object 8 true Other Other undefined object 3 3 5 Hidden,object,8,object 8 Other Hidden 11 11
import QtQuick
import "qmldir_names"
import "qmldir_names" as D
Item {
    id: root
    Other { id: o }
    Plain { id: p }
    UseHid { id: u }
    function w() { Renamed.v = 11; return Renamed.v + " " + Uno.v }
    property string result
    Component.onCompleted: result = [typeof Renamed, Renamed.v, typeof Uno, Uno.v, Renamed === Uno, String(Other), String(Plain),
        typeof Hidden, typeof Hid, o.p, p.p, u.r, u.s, D.Renamed.v, String(D.Other), String(D.Hidden), w()].join(" ")
}
