import QtQuick

Rectangle {
    id: page
    width: 10
    height: 20
    color: "red"; border.width: 1
    border { color: "blue" }
    property int a: 1; property int b: 2
    property int c
    property var d: 1;
    readonly property int e: 2
    default property var f
    required property int g
    required a
    property list<int> li
    property list<Item> lo: [Item {}]
    property var fn: function named(x) { return x }
    states: State { name: "s"; PropertyChanges { target: page; width: 5 } }
    transitions: [
        Transition { from: "*"; to: "s"; NumberAnimation { properties: "width" } },
    ]
    gradient: Gradient {
        GradientStop { position: 0; color: "white" }
        GradientStop { position: 1; color: "black" }
    }
    MouseArea {
        anchors.fill: parent
        onClicked: mouse => { page.a = mouse.x }
        onPressed: page.b = 1
        onReleased: {
            page.c = 2
        }
    }
    Repeater {
        model: 3
        delegate: Text { required property int index; text: index }
    }
    NumberAnimation on opacity { from: 0; to: 1 }
    Keys.onPressed: event => event.accepted = true
    Component { id: comp; Item {} }
    Loader { sourceComponent: comp; active: page.a > 0 }
}
