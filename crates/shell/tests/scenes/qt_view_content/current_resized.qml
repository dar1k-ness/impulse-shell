// qt: start: a y0 h 0 cur -1 none; b y0 h 200 cur 0@0; c y0 h 0 cur -1 none | open: a y0 h 124 cur 0@0; b y200 h 200 cur 0@0; c y0 h 124 cur 0@0 | close: a y0 h 0 cur -1 none; b y0 h 400 cur 0@0; c y0 h 0 cur -1 none | reopen: a y0 h 124 cur 2@84; b y0 h 400 cur 0@0; c y0 h 124 cur 2@84
import QtQuick
import QtQuick.Controls
Item {
    id: root
    width: 300; height: 400
    property string result: ""
    property var steps: ["open", "close", "reopen"]
    property int k: -1
    property var out: []
    function at(view) {
        const c = view.currentItem
        return "y" + Math.round(view.contentY) + " h " + Math.round(view.height) + " cur " + view.currentIndex + (c ? "@" + c.y : " none")
    }
    component Row: Item {
        width: 100; height: 40
    }
    // A popup's list: as tall as its rows, which it has only while open; the row chosen
    // before is made current once it has them.
    ListView {
        id: a
        width: 100; height: contentHeight; spacing: 2
        model: null
        delegate: Row {}
    }
    // A view at the end of its rows that grows to hold them all.
    ListView {
        id: b
        x: 100; width: 100; height: 200
        model: 10
        delegate: Row {}
    }
    // The list of a ComboBox's popup, as StyledComboBox has it.
    ComboBox {
        id: c
        x: 200; width: 100; height: 40
        model: ["one", "two", "three"]
        delegate: ItemDelegate {
            width: 100; height: 40
            required property var model
            text: model[c.textRole]
        }
        popup: Popup {
            y: c.height; width: c.width; padding: 8
            height: Math.min(list.contentHeight + topPadding + bottomPadding, 300)
            contentItem: ListView {
                id: list
                clip: true
                implicitHeight: contentHeight; spacing: 2
                model: c.popup.visible ? c.delegateModel : null
                currentIndex: c.highlightedIndex
            }
        }
    }
    Timer {
        interval: 150; repeat: true; running: true
        onTriggered: {
            root.out.push((root.k < 0 ? "start" : root.steps[root.k]) + ": a " + root.at(a) + "; b " + root.at(b) + "; c " + root.at(list))
            root.k++
            if (root.k < root.steps.length) {
                const s = root.steps[root.k]
                if (s === "open") {
                    a.model = 3
                    a.currentIndex = 0
                    b.contentY = 200
                    c.popup.open()
                } else if (s === "close") {
                    a.currentIndex = -1
                    a.model = null
                    b.height = 400
                    c.currentIndex = 2
                    c.popup.close()
                } else {
                    a.model = 3
                    a.currentIndex = 2
                    c.popup.open()
                }
            } else {
                running = false
                root.result = root.out.join(" | ")
            }
        }
    }
}
