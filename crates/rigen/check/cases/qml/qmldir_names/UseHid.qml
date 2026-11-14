// A file of the directory: it names the `internal` type of its `qmldir`.
import QtQuick
Item {
    Hidden { id: h }
    property int r: h.hd
    property string s: String(Hidden) + "," + typeof Renamed + "," + Renamed.v + "," + typeof Uno
}
