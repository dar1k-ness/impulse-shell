// Qt's side of `an_unchanged_frame_is_not_committed_and_the_transition_ends_on_time`
// (crates/shell/tests/frame_callback_wait.rs): an item fades out in 100 ms and keeps moving
// for 500 ms, so the last 400 ms change no pixel. unchanged_tail.sh runs it.
import QtQuick

Window {
    id: win
    visible: true
    width: 64; height: 32
    color: "#ff0000"
    property real t0
    property int steps
    property int runs
    Rectangle {
        id: dot; width: 4; height: 4; y: 28; color: "#0000ff"
        onXChanged: { win.steps++; console.log("STEP " + x) }
        onOpacityChanged: if (opacity === 0) console.log("GONE")
    }
    ParallelAnimation {
        id: leave
        NumberAnimation { target: dot; property: "opacity"; from: 1; to: 0; duration: 100 }
        NumberAnimation { target: dot; property: "x"; from: 0; to: 60; duration: 500 }
        onRunningChanged: console.log("RUNNING " + running)
        onStarted: { win.t0 = Date.now(); console.log("STARTED") }
        onStopped: console.log("STOPPED")
        onFinished: {
            console.log("FINISHED at " + dot.x + " opacity " + dot.opacity + " after "
                + (Date.now() - win.t0) + " ms, " + win.steps + " steps")
            if (++win.runs == 3)
                Qt.quit()
            else
                again.start()
        }
    }
    Timer { id: again; interval: 700; running: true; onTriggered: { win.steps = 0; leave.start() } }
}
