pragma Singleton
import QtQuick
QtObject {
    property int base: 10
    property int back: Alpha.writes
    Component.onCompleted: console.log("Beta completed, base", base, "Alpha.writes", Alpha.writes)
}
