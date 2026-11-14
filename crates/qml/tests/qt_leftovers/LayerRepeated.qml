import QtQuick

// The background of the shell's RippleButton, six times: each copy has its own effect item.
// The effect is a ShaderEffect with the property OpacityMask has: the Qt the fixtures run in
// has no Qt5Compat.GraphicalEffects.
Item {
    id: root
    width: 200; height: 200
    property int made: 0
    function kids(item) {
        var names = [];
        for (var i = 0; i < item.children.length; i++) {
            var s = item.children[i].toString();
            names.push(s.slice(0, s.search(/[_(]/)));
        }
        return names.join(" ");
    }
    Column {
        Repeater {
            id: buttons
            model: 6
            Item {
                id: button
                width: 40; height: 20
                property real corner: 4
                Rectangle {
                    id: background
                    width: 40; height: 20
                    radius: button.corner
                    layer.enabled: true
                    layer.effect: ShaderEffect {
                        Component.onCompleted: root.made++
                        property var source
                        property var maskSource: Rectangle {
                            width: background.width
                            height: background.height
                            radius: button.corner
                        }
                    }
                }
            }
        }
    }
    Component.onCompleted: {
        console.log("made", made);
        for (var i = 0; i < buttons.count; i++) {
            var b = buttons.itemAt(i);
            var fx = b.children[b.children.length - 1];
            console.log(i, kids(b), b.children[0].width, b.children[0].height, fx.width, fx.height, fx.maskSource.radius, fx.source === b.children[1]);
        }
        Qt.quit();
    }
}
