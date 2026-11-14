// qt: open1 open6 open.decl clipped1 flick.decl@20,20 flick.decl@20,210
import QtQuick
Item {
    id: root
    width: 760; height: 400
    property string result: ""
    property var hits: []
    // Root coordinates, clicked in this order once the views are scrolled.
    property var clicks: [
        [70, 35],   // open list: the row under the view's top edge
        [70, 235],  // open list: the last row in view, on its part below the view
        [70, 250],  // open list: the next row, kept for cacheBuffer and not shown
        [170, 220], // open list: the declared child, which does not scroll
        [330, 35],  // clipped list: the row under the top edge
        [330, 235], // clipped list: the last row's part below the view
        [520, 60],  // flickable: its declared child, scrolled
        [520, 250], // flickable without clip: the child's part below the view
    ]
    ListView {
        id: open
        x: 20; y: 30; width: 200; height: 200
        model: 8
        delegate: MouseArea { width: 150; height: 40; onClicked: root.hits.push("open" + index) }
        MouseArea { x: 100; y: 180; width: 100; height: 20; onClicked: root.hits.push("open.decl") }
    }
    ListView {
        id: clipped
        x: 260; y: 30; width: 200; height: 200
        clip: true
        model: 8
        delegate: MouseArea { width: 150; height: 40; onClicked: root.hits.push("clipped" + index) }
    }
    Flickable {
        id: flick
        x: 500; y: 30; width: 200; height: 200
        contentWidth: 200; contentHeight: 600
        MouseArea { x: 0; y: 80; width: 100; height: 250; onClicked: mouse => root.hits.push("flick.decl@" + mouse.x + "," + mouse.y) }
    }
    Timer { interval: 100; running: true; onTriggered: { open.contentY = 70; clipped.contentY = 70; flick.contentY = 70 } }
    Timer { interval: 700; running: true; onTriggered: root.result = root.hits.join(" ") }
}
