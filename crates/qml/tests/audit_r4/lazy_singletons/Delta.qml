pragma Singleton
import QtQuick
QtObject {
    property int d: 4
    Component.onCompleted: { console.log("Delta completed"); Alpha.writes = 7 }
    Component.onDestruction: console.log("Delta destruction")
}
