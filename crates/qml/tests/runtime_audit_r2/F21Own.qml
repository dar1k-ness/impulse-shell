import QtQuick
Rectangle {
    property int own: 1
    function inside() { return String(this).replace(/0x[0-9a-f]+/g, "0x").replace(/_(QMLTYPE|QML)_\d+/g, "_$1_N") }
}
