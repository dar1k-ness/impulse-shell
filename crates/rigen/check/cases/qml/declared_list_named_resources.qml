// A QML `property list<var> resources` on an Item is a list of its own, not QQuickItem::resources
// (the item's non-Item children): as the tabs of ii's Resources overlay widget, a Repeater over
// its length and a required property bound to a field of the current element. At creation, and
// after the property an element reads is written.
// qt: 3 CPU,RAM,Swap 3 50.0% 4 GHz; 3 CPU,RAM,Swap 3 75.0% 8 GHz; 3 CPU,RAM,Swap 3 75.0% 8 GHz; 
import QtQuick
Item {
    id: root
    property string result: ""
    QtObject {
        id: usage
        property list<real> cpu: [0.25, 0.5]
        property string max: "4 GHz"
    }
    property list<var> resources: [
        { "name": "CPU", "history": usage.cpu, "max": usage.max },
        { "name": "RAM", "history": usage.cpu, "max": usage.max },
        { "name": "Swap", "history": usage.cpu, "max": usage.max },
    ]
    Item {
        id: bar
        property int currentIndex: 0
        Repeater {
            id: tabs
            model: root.resources.length
            delegate: Item {
                required property int index
                property var modelData: root.resources[index]
                property string name: modelData.name
            }
        }
    }
    component Summary: Item {
        id: summary
        required property list<real> history
        required property string max
        property string text: (summary.history[summary.history.length - 1] * 100).toFixed(1) + "%"
    }
    Summary {
        id: shown
        history: root.resources[bar.currentIndex]?.history ?? []
        max: root.resources[bar.currentIndex]?.max ?? "--"
    }
    function show() {
        var names = []
        for (var i = 0; i < tabs.count; i++)
            names.push(tabs.itemAt(i).name)
        result += tabs.count + " " + names.join(",") + " " + resources.length + " " + shown.text + " " + shown.max + "; "
    }
    Component.onCompleted: {
        show()
        usage.cpu = [0.75]
        usage.max = "8 GHz"
        show()
        bar.currentIndex = 2
        show()
    }
}
