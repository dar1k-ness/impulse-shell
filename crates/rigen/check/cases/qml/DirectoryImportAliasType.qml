// The type directory_import_alias.qml names through its `import "." as Here`.
import QtQuick
QtObject {
    enum Mode { A, B, C }
    property int n: 5
    property string result: "type " + n
}
