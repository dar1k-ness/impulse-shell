import QtQuick
Item {
    id: root
    property int _x: 0
    property int hits: 0
    on_XChanged: { hits++; console.log("handler run", hits) }
    Component.onCompleted: { console.log("completed, hits", hits); _x = 5; _x = 6; console.log("end, hits", hits); console.log("END"); Qt.quit() }
}
