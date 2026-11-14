import QtQuick
Item {
    id: root
    signal s1()
    signal s2()
    signal s3()
    property var cb: null
    property var cb2: null
    onS1: root.cb = function() { console.log("cb called at assignment!") }
    onS2: { console.log("s2"); root.cb2 = () => { console.log("cb2 called at assignment!") } }
    onS3: { if (true) { var f = function() { console.log("f called") } } }
    Timer { id: t; interval: 10; onTriggered: root.cb = () => console.log("timer cb called") }
    Component.onCompleted: { s1(); s2(); s3(); console.log("done"); t.start() }
    Timer { interval: 50; running: true; onTriggered: { console.log("END"); Qt.quit() } }
}
