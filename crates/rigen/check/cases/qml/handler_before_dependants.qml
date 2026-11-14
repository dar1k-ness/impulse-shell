// A change handler runs before the bindings that read the changed property are updated, and a
// write it makes updates only what reads the written property: the handler of `ready` reads
// the old `automatic`, and `automatic` is updated after that handler returns, not inside the
// handler's first assignment. (The night light schedule of qml/services/Hyprsunset.qml:
// 6000 is "off", 5000 is "on"; the last one sent has to agree with `active`.)
// qt: ready(auto=false,twice=1) 6000 auto 5000 5000 twice | ready(auto=false,twice=1) auto(true) twice | 6000 5000 5000 active=true
import QtQuick
Item {
    id: root
    property string result: ""

    property bool src: false
    property bool ready: src
    property bool automatic: true && ready
    property int twice: ready ? 2 : 1
    property bool on: false
    property bool first: true
    property bool active: false
    property string sent: ""
    onReadyChanged: {
        result += "ready(auto=" + automatic + ",twice=" + twice + ") "
        if (!automatic)
            ensure()
    }
    onAutomaticChanged: {
        result += "auto "
        first = true
        on = true
        if (first) {
            first = false
            ensure()
        }
    }
    onTwiceChanged: if (twice == 2) result += "twice "
    onOnChanged: ensure()
    function ensure() {
        if (!automatic) {
            if (ready)
                disable()
            return
        }
        if (on)
            enable()
        else
            disable()
    }
    function enable() {
        active = true
        send(5000)
    }
    function disable() {
        active = false
        send(6000)
    }
    function send(t) {
        sent += t + " "
        result += t + " "
    }

    // The same without a write in the handler.
    QtObject {
        id: plain
        property string log: ""
        property bool src: false
        property bool ready: src
        property bool automatic: true && ready
        property int twice: ready ? 2 : 1
        onReadyChanged: log += "ready(auto=" + automatic + ",twice=" + twice + ") "
        onAutomaticChanged: log += "auto(" + automatic + ") "
        onTwiceChanged: if (twice == 2) log += "twice"
    }

    Component.onCompleted: {
        src = true
        plain.src = true
        result += "| " + plain.log + " | " + sent + "active=" + active
    }
}
