import QtQuick
Item {
    property alias loader: l
    function own(u) { l.setSource(u) }
    Loader { id: l }
}
