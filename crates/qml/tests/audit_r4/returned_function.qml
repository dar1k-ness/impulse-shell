import QtQuick
Item {
    id: root
    signal s1(int a)
    signal s2(int a)
    signal s3(int a)
    signal s4(int a)
    property var cb: function(a) { console.log("cb called", a) }
    function foo(a) { console.log("foo called", a) }
    function mk() { console.log("mk"); return foo }
    onS1: root.cb
    onS2: { foo }
    onS3: mk()
    onS4: { mk(); }
    Component.onCompleted: { s1(1); s2(2); s3(3); s4(4); console.log("done"); console.log("END"); Qt.quit() }
}
