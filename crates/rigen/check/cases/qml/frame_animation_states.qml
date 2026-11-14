// QQuickFrameAnimation through its states. `triggered` comes once per step of the animation
// clock; the first one after a start sets `currentFrame` to 0 when it was above 0 and to 1
// when it was 0; `smoothFrameTime` is 0.1 of the new `frameTime` plus 0.9 of what it was and
// is never cleared; `reset()` clears `elapsedTime` and `currentFrame` only; a start clears
// `elapsedTime` without `elapsedTimeChanged`; every method reports its changes before it
// returns.
// A step is `E+ F<n> t<stage> <n> +se`: elapsedTimeChanged, currentFrameChanged, then the
// handler, which saw frameTime above 0 (`+`), smoothFrameTime as the rule above gives it (`s`)
// and elapsedTime grown by frameTime (`e`). Any other line is `<what> <currentFrame>
// <frameTime><smoothFrameTime><elapsedTime> <running><paused>`, each time as `0`, `=` (as in
// the line before) or `~`. R, P, F, E are the changed signals.
// The line below is Qt 6.12's `result` (crates/qml/tests/frame_animation.rs asks Qt again).
// qt: completed 0 000 r- E+ F1 t0 1 +se E+ F2 t0 2 +se E+ F3 t0 3 +se E+ F4 t0 4 +se E+ F5 t0 5 +se E+ F6 t0 6 +se E+ F7 t0 7 +se E+ F8 t0 8 +se E+ F9 t0 9 +se E+ F10 t0 10 +se P1 pause 10 === rp later 10 === rp P0 resume 10 === r- E+ F11 t1 11 +se E+ F12 t1 12 +se E+ F13 t1 13 +se E0 F0 reset 0 ==0 r- E+ F1 t2 1 +se E+ F2 t2 2 +se E+ F3 t2 3 +se R0 R1 restart 3 ==0 r- E+ F0 t3 0 +se E+ F1 t3 1 +se E+ F2 t3 2 +se R0 stop 2 === -- R1 start 2 ==0 r- E+ F0 t4 0 +se E+ F1 t4 1 +se E+ F2 t4 2 +se R0 off 2 === -- later 2 === -- R1 on 2 ==0 r- E+ F0 t5 0 +se E+ F1 t5 1 +se E+ F2 t5 2 +se P1 E0 F0 pause,reset 0 ==0 rp later 0 ==0 rp P0 resume 0 ==0 r- E+ F1 t6 1 +se E+ F2 t6 2 +se E+ F3 t6 3 +se R0 R1 P1 restart,pause 3 ==0 rp later 3 ==0 rp P0 resume 3 ==0 r- E+ F0 t7 0 +se E+ F1 t7 1 +se E+ F2 t7 2 +se P1 R0 P0 pause,stop 2 === -- E0 F0 reset 0 ==0 -- R1 start 0 ==0 r- E+ F1 t8 1 +se E+ F2 t8 2 +se E+ F3 t8 3 +se P1 paused 3 === rp later 3 === rp P0 unpaused 3 === r- E+ F4 t9 4 +se E+ F5 t9 5 +se E+ F6 t9 6 +se R0 stop 6 === --
import QtQuick
Item {
    id: root
    property string result: ""
    property var got: []
    property int stage: 0
    property int remaining: 10
    property real ft: 0
    property real sm: 0
    property real el: 0
    function part(now, was) { return now == 0 ? "0" : now == was ? "=" : "~" }
    function seen() { ft = fa.frameTime; sm = fa.smoothFrameTime; el = fa.elapsedTime }
    function line(what) {
        got.push(what + " " + fa.currentFrame + " " + part(fa.frameTime, ft) + part(fa.smoothFrameTime, sm)
                 + part(fa.elapsedTime, el) + " " + (fa.running ? "r" : "-") + (fa.paused ? "p" : "-"))
        seen()
    }
    function step() {
        var s = Math.abs(fa.smoothFrameTime - (0.1 * fa.frameTime + 0.9 * sm)) < 1e-9
        var e = Math.abs(fa.elapsedTime - (el + fa.frameTime)) < 1e-9
        got.push("t" + stage + " " + fa.currentFrame + " " + (fa.frameTime > 0 ? "+" : "0") + (s ? "s" : "S") + (e ? "e" : "E"))
        seen()
    }
    FrameAnimation {
        id: fa
        running: true
        onRunningChanged: root.got.push("R" + (running ? 1 : 0))
        onPausedChanged: root.got.push("P" + (paused ? 1 : 0))
        onCurrentFrameChanged: root.got.push("F" + currentFrame)
        onElapsedTimeChanged: root.got.push("E" + (elapsedTime == 0 ? "0" : "+"))
        onTriggered: {
            root.step()
            if (--root.remaining > 0)
                return
            root.remaining = 3
            switch (++root.stage) {
            case 1: fa.pause(); root.line("pause"); wait.start(); break
            case 2: fa.reset(); root.line("reset"); break
            case 3: fa.restart(); root.line("restart"); break
            case 4: fa.stop(); root.line("stop"); fa.start(); root.line("start"); break
            case 5: fa.running = false; root.line("off"); wait.start(); break
            case 6: fa.pause(); fa.reset(); root.line("pause,reset"); wait.start(); break
            case 7: fa.restart(); fa.pause(); root.line("restart,pause"); wait.start(); break
            case 8: fa.pause(); fa.stop(); root.line("pause,stop"); fa.reset(); root.line("reset"); fa.start(); root.line("start"); break
            case 9: fa.paused = true; root.line("paused"); wait.start(); break
            case 10: fa.stop(); root.line("stop"); root.result = root.got.join(" "); Qt.quit(); break
            }
        }
    }
    Timer {
        id: wait
        interval: 100
        onTriggered: {
            root.line("later")
            if (root.stage == 5) { fa.running = true; root.line("on") }
            else if (root.stage == 9) { fa.paused = false; root.line("unpaused") }
            else { fa.resume(); root.line("resume") }
        }
    }
    Component.onCompleted: root.line("completed")
}
