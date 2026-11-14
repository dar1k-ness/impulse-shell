import QtQuick
Item {
    Instantiator { active: false; delegate: QtObject { property var o: Item { Uses {} } } }
}
