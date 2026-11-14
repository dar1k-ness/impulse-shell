// QQuickFrameAnimation is not an item and does not look at the one it is declared in: it is
// triggered at every step of the animation clock in an item whose `visible` is false, under
// a parent whose `visible` is false, in a Window whose `visible` is false, and in an item
// hidden while it runs, and its `elapsedTime` counts that time.
// The jobs step in the order they were started: the declared ones in the order their
// components complete (the last declared first), one started again after the others.
// A line is `<where> <triggers so far> F<currentFrame> <frameTime above 0: +><elapsedTime as
// the shown one's: e, else E> <running><paused>`, taken in the handler of the shown one, which
// is declared first and so steps last. `order` is who was triggered since its last trigger.
// The line below is Qt 6.12's `result` (crates/qml/tests/frame_animation.rs asks Qt again).
// qt: order dcbas visible false false false item 5 F5 +e r- parent 5 F5 +e r- window 5 F5 +e r- later 5 F5 +e r- hidden 10 F10 +e r- again 15 F15 +e r- item 15 F15 +e r- parent 15 F15 +e r- window 15 F15 +e r- order badcs item 16 F16 +E r- parent 16 F0 +E r-
import QtQuick
Item {
    id: root
    property string result: ""
    property var got: []
    property int a: 0
    property int b: 0
    property int c: 0
    property int d: 0
    property string order: ""
    function near(x, y) { return Math.abs(x - y) < 0.008 }
    function line(what, n, f) {
        got.push(what + " " + n + " F" + f.currentFrame + " " + (f.frameTime > 0 ? "+" : "0")
                 + (near(f.elapsedTime, shown.elapsedTime) ? "e" : "E") + " " + (f.running ? "r" : "-") + (f.paused ? "p" : "-"))
    }
    FrameAnimation {
        id: shown
        running: true
        onTriggered: {
            root.order += "s"
            if (currentFrame == 1 || currentFrame == 17)
                root.got.push("order " + root.order)
            root.order = ""
            if (currentFrame == 5) {
                root.got.push("visible " + hidden.visible + " " + child.visible + " " + win.visible)
                root.line("item", root.a, fa)
                root.line("parent", root.b, fb)
                root.line("window", root.c, fc)
                root.line("later", root.d, fd)
                later.visible = false
            } else if (currentFrame == 10) {
                root.line("hidden", root.d, fd)
                later.visible = true
            } else if (currentFrame == 15) {
                root.line("again", root.d, fd)
                root.line("item", root.a, fa)
                root.line("parent", root.b, fb)
                root.line("window", root.c, fc)
                fb.restart()
                fa.pause()
                fa.resume()
            } else if (currentFrame == 17) {
                root.line("item", root.a, fa)
                root.line("parent", root.b, fb)
                root.result = root.got.join(" ")
                Qt.quit()
            }
        }
    }
    Item {
        id: hidden
        visible: false
        FrameAnimation { id: fa; running: true; onTriggered: { root.a++; root.order += "a" } }
        Item {
            id: child
            FrameAnimation { id: fb; running: true; onTriggered: { root.b++; root.order += "b" } }
        }
    }
    Window {
        id: win
        visible: false
        FrameAnimation { id: fc; running: true; onTriggered: { root.c++; root.order += "c" } }
    }
    Item {
        id: later
        FrameAnimation { id: fd; running: true; onTriggered: { root.d++; root.order += "d" } }
    }
}
