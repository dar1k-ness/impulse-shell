pragma Singleton
import QtQuick
QtObject {
    property int g: 1
    Component.onCompleted: console.log("Gamma completed (never referenced by value)")
}
