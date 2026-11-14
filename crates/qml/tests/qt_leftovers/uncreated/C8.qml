import QtQuick
Item {
    Component {
        Item {
            id: d
            states: State { PropertyChanges { target: d } Nope8 {} }
        }
    }
}
