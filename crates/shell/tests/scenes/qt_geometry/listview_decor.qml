import QtQuick
Item {
    id: root; width: 400; height: 400
    ListView { objectName: "lv1"; width: 100; height: 95; spacing: 3; model: 4
        delegate: Rectangle { objectName: "lv1d" + index; width: ListView.view.width; height: 20 + (index % 2) * 4 }
        header: Rectangle { objectName: "lv1h"; width: 50; height: 9 }
        footer: Rectangle { objectName: "lv1f"; width: 50; height: 7 }
    }
    ListView { id: lv2; objectName: "lv2"; x: 120; width: 200; height: 30; orientation: ListView.Horizontal; spacing: 2; model: ["a", "b", "c"]; currentIndex: 2
        delegate: Rectangle { objectName: "lv2d" + index; width: 30 + index * 5; height: 10 }
        highlight: Rectangle { objectName: "lv2hl"; color: "blue" }
    }
    ListView { objectName: "lv3"; x: 120; y: 50; width: 60; height: 100; spacing: 1; model: 3; currentIndex: 1
        delegate: Rectangle { objectName: "lv3d" + index; width: 40; height: 10 + index * 2 }
        highlight: Rectangle { objectName: "lv3hl"; color: "blue" }
    }
    ListView { objectName: "lv4"; y: 200; x: 100; width: 80; height: 60; model: 3; verticalLayoutDirection: ListView.BottomToTop
        delegate: Rectangle { objectName: "lv4d" + index; width: 20; height: 10 } }
    ListView { objectName: "lv5"; y: 200; x: 200; width: 80; height: 90; spacing: 2; model: 3; verticalLayoutDirection: ListView.BottomToTop
        delegate: Rectangle { objectName: "lv5d" + index; width: 20; height: 10 + index }
        header: Rectangle { objectName: "lv5h"; width: 30; height: 5 }
        footer: Rectangle { objectName: "lv5f"; width: 30; height: 4 }
    }
    ListView { objectName: "lv6"; y: 320; width: 300; height: 20; orientation: ListView.Horizontal; spacing: 2; model: 3
        delegate: Rectangle { objectName: "lv6d" + index; width: 30; height: 10 }
        header: Rectangle { objectName: "lv6h"; width: 12; height: 10 }
        footer: Rectangle { objectName: "lv6f"; width: 8; height: 10 }
    }
}
