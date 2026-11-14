// qt: 0,0 contentXY=0,0 | 3,16 contentXY=-3,-16 | 3,16 contentXY=-3,-16 | 5,7 contentXY=-5,-7 | 5,0 contentXY=-5,0 | 0,-20 contentXY=0,20 || 0,0 contentXY=0,0 | 3,16 contentXY=-3,-16 | 3,16 contentXY=-3,-16 | 5,7 contentXY=-5,-7 | 5,0 contentXY=-5,0 | 0,-20 contentXY=0,20
import QtQuick
Item {
    id: root
    width: 300; height: 400
    property string result: ""
    Flickable { id: a; width: 120; height: 90 }
    Flickable { id: c; width: root.width; height: root.height / 2; contentHeight: height * 3; topMargin: 16; leftMargin: 3 }
    Flickable { id: g; topMargin: 16; leftMargin: 3 }
    Flickable { id: f; width: 80; height: 100; topMargin: 7; leftMargin: 5; contentHeight: 300 }
    Flickable { id: f2; width: 80; height: 100; topMargin: 7; leftMargin: 5; contentHeight: 300; contentY: 0 }
    Flickable { id: f3; width: 80; height: 100; contentHeight: 300; contentY: 20 }
    Component.onCompleted: {
        const g1 = v => v.contentItem.x + "," + v.contentItem.y + " contentXY=" + v.contentX + "," + v.contentY
        const all = () => [a, c, g, f, f2, f3].map(g1).join(" | ")
        let s = all()
        f3.topMargin = 9
        result = s + " || " + all()
    }
}
