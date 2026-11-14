import QtQuick
import QtQml
Item {
    id: root
    Instantiator { id: two; model: 2; delegate: QtObject { }
        onActiveChanged: console.log("activeChanged", active)
        onCountChanged: console.log("countChanged", count) }
    Component.onCompleted: { console.log("A", two.count); two.active = false; console.log("B", two.count); two.active = true; console.log("C", two.count); console.log("END"); Qt.quit() }
}
