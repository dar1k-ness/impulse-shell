import QtQuick
import Rustshell
FloatingWindow {
    width: 160; height: 64; color: "#202020"
    Item { x: 60; y: 20; width: 20; height: 10
        Rectangle { anchors.left: parent.right; anchors.right: parent.left; height: 10; color: "#ff0000" }
        Rectangle { anchors.fill: parent; anchors.margins: 12; color: "#ff0000"; border.width: 2; border.color: "#0000ff"; radius: 3 }
        Item { anchors.left: parent.right; anchors.right: parent.left; height: 10; clip: true
            Rectangle { x: -20; width: 20; height: 10; color: "#ff00ff" } }
        Item { anchors.left: parent.right; anchors.right: parent.left; height: 10
            Rectangle { x: -20; y: 12; width: 6; height: 6; color: "#00ffff" } }
        Text { anchors.left: parent.right; anchors.right: parent.left; y: 20; text: "WWWW"; color: "#ffff00"; font.pixelSize: 12; renderType: Text.NativeRendering }
        Text { anchors.left: parent.right; anchors.right: parent.left; y: -20; text: "WW WW"; color: "#00ff00"; font.pixelSize: 12; renderType: Text.NativeRendering; wrapMode: Text.Wrap }
        Text { anchors.left: parent.right; anchors.right: parent.left; y: 20; text: "WWWW"; color: "#ffffff"; font.pixelSize: 12; renderType: Text.NativeRendering; elide: Text.ElideRight }
        Text { x: -50; y: -20; width: 0; text: "WWWW"; color: "#ffffff"; font.pixelSize: 12; renderType: Text.NativeRendering; elide: Text.ElideMiddle }
        Text { id: used; x: -55; y: 20; width: 0; text: "WW"; color: "#ff8000"; font.pixelSize: 12; renderType: Text.NativeRendering; elide: Text.ElideRight }
        Item { width: used.implicitWidth }
    }
}
