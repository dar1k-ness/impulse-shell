import QtQuick
// What a changed-handler sees of the bindings that depend on the changed property: a write from
// JS, from a binding that changed (the same write), and from an animation step.
Item {
    id: root
    property string log: ""
    function say(...a) { const s = a.join(" "); log += s + "\n"; console.log(s) }
    property bool src: false
    property bool early: configReady
    property bool configReady: src
    property bool opt: true
    property bool automatic: opt && configReady
    property bool other: configReady
    property int w: src ? 10 : 0
    property int h: src ? 20 : 0
    property real a: 0
    property real a2: a * 2
    property real a3: a2 + 1
    property int frame: 0
    onSrcChanged: say("src", src, "ready", configReady, "w", w, "h", h)
    onConfigReadyChanged: say("ready", configReady, "automatic", automatic, "other", other, "early", early)
    onEarlyChanged: say("early", early, "automatic", automatic, "other", other)
    onAutomaticChanged: say("automatic", automatic, "other", other, "early", early)
    onOtherChanged: say("other", other, "automatic", automatic, "early", early)
    onWChanged: say("w", w, "h", h)
    onHChanged: say("h", h, "w", w)
    onAChanged: say("a", a.toFixed(0), "a2", a2.toFixed(0), "a3", a3.toFixed(0))
    onA2Changed: say("a2", a2.toFixed(0), "a", a.toFixed(0), "a3", a3.toFixed(0))
    onA3Changed: say("a3", a3.toFixed(0), "a", a.toFixed(0), "a2", a2.toFixed(0))
    NumberAnimation { target: root; property: "a"; from: 0; to: 64; duration: 64; running: true }
    function sample() {
        frame++
        if (frame === 6) { say("write"); src = true; say("written", automatic, w, h) }
        if (frame === 8) { say("back"); src = false; say("written", automatic, w, h) }
    }
}
