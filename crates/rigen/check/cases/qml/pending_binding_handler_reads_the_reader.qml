// qt: onB(2,0) onA(3) | 3 2 ran 1,1 | onB(6,3) onA(7) | 7 6 ran 2,2
import QtQuick
Item {
    id: root
    property string result: ""
    // How many times each binding ran.
    property var ran: ({ a: 0, b: 0 })
    function log(s) { result += s }
    function counted(name, v) { ran[name]++; return v }

    // `a` is first: its binding reads `b` while the binding of `b` has not run yet. `b` gets
    // its value inside that read and `onBChanged` runs there, reading `a`, whose binding is
    // the one that runs. That read is the handler's, not the binding's: `a` does not come to
    // depend on itself.
    property int c: 1
    property int a: counted("a", b + 1)
    property int b: counted("b", c + 1)
    onBChanged: log("onB(" + b + "," + a + ") ")
    onAChanged: log("onA(" + a + ") ")

    Component.onCompleted: {
        log("| " + a + " " + b + " ran " + ran.a + "," + ran.b + " | ")
        c = 5
        log("| " + a + " " + b + " ran " + ran.a + "," + ran.b)
    }
}
