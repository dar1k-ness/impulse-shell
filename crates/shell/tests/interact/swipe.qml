// A SwipeView with nothing else on the window that animates: the frames that show its drag and
// its settling after the release are the ones the view itself asks for.
import QtQuick
import QtQuick.Controls
import Quickshell
import Quickshell.Wayland

ShellRoot {
    PanelWindow {
        id: win
        anchors.top: true
        anchors.left: true
        implicitWidth: 1200
        implicitHeight: 760
        exclusionMode: ExclusionMode.Ignore
        WlrLayershell.namespace: "quickshell:iact"
        // As in shell.qml: sway gives a layer that takes the keyboard its focus when it maps and
        // looks up what is under the pointer then, so the parked pointer enters without moving.
        WlrLayershell.keyboardFocus: WlrKeyboardFocus.OnDemand
        color: "#202020"

        // The test's input clock, as in shell.qml.
        FrameAnimation {
            running: true
            onTriggered: console.log("ITICK")
        }

        MouseArea {
            id: enterProbe
            anchors.fill: parent
            z: -1
            hoverEnabled: true
            acceptedButtons: Qt.NoButton
            onContainsMouseChanged: {
                if (containsMouse)
                    console.log("IACT ready");
            }
        }

        SwipeView {
            id: swipe
            x: 950; y: 340; width: 200; height: 120
            clip: true
            onCurrentIndexChanged: {
                settle.frames = 0;
                settle.restart();
            }
            Rectangle { id: page0; color: "#a33" }
            Rectangle { color: "#3a3" }
        }

        // Where the view is once it stopped on its current page, or `stuck` with where it still
        // is 600 frames after the page changed (the move to a page takes under 20).
        FrameAnimation {
            id: settle
            property int frames: 0
            onTriggered: {
                const o = -page0.mapToItem(swipe, 0, 0).x;
                const moving = !!(swipe.contentItem && swipe.contentItem.moving);
                if (!moving && Math.abs(o - swipe.currentIndex * swipe.width) < 0.5) {
                    stop();
                    console.log("IACT swipe rest " + Math.round(o));
                } else if (++frames == 600) {
                    stop();
                    console.log("IACT swipe stuck " + Math.round(o));
                }
            }
        }
    }
}
