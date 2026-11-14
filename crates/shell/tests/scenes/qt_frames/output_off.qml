// Qt's side of `a_transition_ends_and_timers_run_with_the_output_off`
// (crates/shell/tests/frame_callback_wait.rs): the same 500 ms animation and 50 ms Timer,
// started 1.5 s after the window is shown. output_off.sh powers the output off at IDLE: the
// window is drawn and its frame callback answered, as the panel of that test is.
import QtQuick

Window {
    id: win
    visible: true
    width: 64; height: 32
    color: "#ff0000"
    property real t0
    property int beats
    property int steps
    Rectangle { id: dot; width: 4; height: 4; y: 28; color: "#0000ff"; onXChanged: win.steps++ }
    NumberAnimation {
        id: move
        target: dot; property: "x"; from: 0; to: 60; duration: 500
        onStarted: win.t0 = Date.now()
        onFinished: {
            console.log("FINISHED at " + dot.x + " after " + (Date.now() - win.t0) + " ms, "
                + win.steps + " steps, " + win.beats + " beats")
            Qt.quit()
        }
    }
    Timer { id: beat; interval: 50; repeat: true; onTriggered: win.beats++ }
    Timer { interval: 700; running: true; onTriggered: console.log("IDLE") }
    Timer { interval: 1500; running: true; onTriggered: { beat.start(); move.start() } }
}
