pragma Singleton
import QtQuick
QtObject {
    id: root
    property int value: Beta.base + 1
    property int writes: 0
    function hello() { return "hello from Alpha" }
    Component.onCompleted: console.log("Alpha completed, value", value, "Beta.base", Beta.base)
}
