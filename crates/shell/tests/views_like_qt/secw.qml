import QtQuick
import Rustshell
FloatingWindow { visible: true; implicitWidth: 200; implicitHeight: 400
Item {
    id: root
    Timer { id: done; interval: 200; onTriggered: { console.log("done"); Qt.quit() } }
    width: 200; height: 400
    property var rows: [ {name: "a", category: "Apps"}, {name: "b", category: "Apps"}, {name: "c", category: "Files"}, {name: "d", category: "apps"}, {name: "e", category: "Apps"}, {name: "f", category: ""}, {name: "g", category: "Web"}, {name: "h", category: "Web"} ]
    ListView {
        id: lv; objectName: "lv"
        width: 200; height: 400; spacing: 4
        model: root.rows
        section { criteria: ViewSection.FullString; property: "category"; labelPositioning: ViewSection.InlineLabels
            delegate: Item { required property string section; objectName: "sec_" + section + "_" + y; width: ListView.view ? ListView.view.width : 0; implicitHeight: 38 + 4; height: implicitHeight
                Component.onCompleted: console.log("section created", section) }
        }
        delegate: Rectangle { required property var modelData; required property int index; objectName: "d" + index; width: 200; height: 20
            Component.onCompleted: console.log("d", index, modelData.name, JSON.stringify(ListView.section), JSON.stringify(ListView.previousSection), JSON.stringify(ListView.nextSection)) }
    }
    ListModel { id: lm
        ListElement { name: "apple"; kind: "fruit" } ListElement { name: "avocado"; kind: "fruit" } ListElement { name: "beet"; kind: "veg" } ListElement { name: "banana"; kind: "fruit" } ListElement { name: "Bean"; kind: "veg" } }
    ListView {
        id: lv2; objectName: "lv2"; x: 0; y: 0
        width: 200; height: 400
        model: lm
        section.property: "name"; section.criteria: ViewSection.FirstCharacter
        section.delegate: Rectangle { required property string section; objectName: "s2_" + section + "_" + y; width: 200; height: 15 }
        delegate: Rectangle { required property string name; required property int index; objectName: "e" + index; width: 200; height: 20
            ListView.onSectionChanged: console.log("e sectionChanged", index, ListView.section) }
    }
    ListView {
        id: lv3; objectName: "lv3"
        width: 200; height: 400
        model: lm
        section.property: "kind"
        delegate: Rectangle { required property string name; required property int index; objectName: "g" + index; width: 200; height: 20 }
    }
    Timer { interval: 200; running: true; onTriggered: {
        console.log("lv", lv.count, lv.contentHeight, lv.currentSection, "lv2", lv2.contentHeight, lv2.currentSection, "lv3", lv3.contentHeight, JSON.stringify(lv3.currentSection))
        lm.setProperty(2, "name", "apricot"); lm.insert(0, {name: "zebra", kind: "animal"}); lm.remove(4)
        console.log("lv2 after", lv2.contentHeight, lv2.count)
        lv.contentY = 100; console.log("currentSection scrolled", lv.currentSection); done.start()
    } }
}
}
