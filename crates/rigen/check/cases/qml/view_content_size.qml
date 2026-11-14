// qt: 120x90 | 300x500 | 297x600 | 80x0 | 0x0 | 0x93 || 120x50 | 350x500 | 297x184 | 60x0 | 0x4 | 0x43
import QtQuick
Item {
    id: root
    width: 300; height: 400
    property string result: ""
    Flickable { id: a; width: 120; height: 90 }
    Flickable { id: b; width: 200; height: 200; contentWidth: 300; contentHeight: 500 }
    Flickable { id: c; width: root.width; height: root.height / 2; contentHeight: height * 3; topMargin: 16; leftMargin: 3 }
    ListView { id: d; width: 80; height: 100 }
    Flickable { id: g; topMargin: 16; leftMargin: 3 }
    ListView { id: h; width: 80; height: 100; topMargin: 7; orientation: ListView.Horizontal }
    Component.onCompleted: {
        const g1 = v => v.contentItem.width + "x" + v.contentItem.height
        const all = () => [a, b, c, d, g, h].map(g1).join(" | ")
        let s = all()
        a.height = 50; b.contentWidth = 350; c.contentHeight = -1; d.width = 60; g.height = 20; h.height = 50
        result = s + " || " + all()
    }
}
