import QtQuick
Item {
    property Item header
    property QtObject footer: QtObject { objectName: "defaultFooter" }
    property var delegate
    property Item highlight
    property Component made
    property alias viaAlias: inner.made2
    QtObject { id: inner; property Component made2 }
}
