import QtQuick
Item {
    id: root
    property string log: ""
    function p(...a) { log += a.join(" ") + "\n"; }
    Timer { id: t; interval: 1000; onRunningChanged: root.p("running", running) }
    Timer { id: idle; interval: 1000; onRunningChanged: root.p("idle running", running) }
    Timer { id: seen; interval: 1000; property bool wasOff: false; onRunningChanged: if (!running) wasOff = true }
    // A binding that restarts a Timer does not come to depend on `running`.
    Timer { id: bt; interval: 1000 }
    property var evals: ({ n: 0 })
    property int q: 0
    property int r: { evals.n++; bt.restart(); return q + 1; }
    Component.onCompleted: {
        t.start(); p("--restart"); t.restart(); p("--end");
        p("--idle"); idle.restart(); p("--end");
        seen.start(); seen.restart(); p("handler saw false", seen.wasOff, seen.running);
        p("--binding", r, evals.n); q = 5; p("binding", r, evals.n); bt.stop(); p("stopped", r, evals.n);
    }
}
