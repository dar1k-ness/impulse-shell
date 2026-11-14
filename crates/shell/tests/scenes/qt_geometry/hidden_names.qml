import QtQuick
Item {
    id: root; width: 400; height: 400
    // A property declared under the name of a C++ one leaves the C++ one as it was: the
    // positioner's gap, the implicit size, the state, the scale, the current index, whether a
    // Loader loads and what a Repeater makes do not follow the declared property.
    Row { objectName: "spacing"; property int spacing: 7
        Rectangle { objectName: "spacing_0"; width: 10; height: 10 }
        Rectangle { objectName: "spacing_1"; width: 10; height: 10 }
    }
    Row { objectName: "implicit"; y: 20
        Item { objectName: "implicit_0"; property real implicitWidth: 50; property real implicitHeight: 20 }
        Rectangle { objectName: "implicit_1"; width: 10; height: 10 }
    }
    Rectangle { id: st; objectName: "state"; y: 50; width: 10; height: 10; property string state: "wide"
        states: State { name: "wide"; PropertyChanges { target: st; width: 30 } }
    }
    Item { id: sc; objectName: "scale"; y: 70; width: 10; height: 10; property real scale: 2 }
    Item { id: probe; objectName: "scale_probe" }
    ListView { objectName: "current"; y: 100; width: 50; height: 50; model: 5; property int currentIndex: 3
        delegate: Item { width: 50; height: 10 }
        highlight: Rectangle { objectName: "current_highlight" }
        highlightFollowsCurrentItem: true; highlightMoveDuration: 0
    }
    Loader { objectName: "loader"; y: 160; property bool active: false
        sourceComponent: Rectangle { objectName: "loader_item"; width: 12; height: 8 }
    }
    Row { objectName: "repeater"; y: 180
        Repeater { property int model: 3; Rectangle { objectName: "repeater_" + index; width: 10; height: 10 } }
    }
    Row { objectName: "repeater_count"; y: 200
        Repeater { property int count: 7; model: 2; Rectangle { objectName: "repeater_count_" + index; width: 10; height: 10 } }
    }
    Item { objectName: "resources"; y: 220; width: childrenRect.width; height: childrenRect.height; property list<var> resources: [1, 2]
        Rectangle { objectName: "resources_0"; width: 9; height: 9 }
    }
    Item { id: en; objectName: "enabled"; y: 240; property bool enabled: false
        Item { id: enk; objectName: "enabled_kid"; width: enabled ? 5 : 3 }
    }
    Grid { id: gr; objectName: "grid_alias"; y: 260; columns: 2; property alias spacing: gr.rowSpacing; spacing: 6
        Repeater { model: 4; Rectangle { objectName: "grid_alias_" + index; width: 10; height: 10 } }
    }
    Component.onCompleted: { const p = sc.mapToItem(root, 10, 10); probe.x = p.x; probe.y = p.y }
}
