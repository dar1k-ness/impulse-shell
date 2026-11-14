import QtQuick

// The handler of a signal is `on` and the signal with its first letter in upper case:
// what stands before the letter stays.
Item {
    id: root
    property int _z
    property int __y
    property int $d
    property int _1q
    property int plain
    signal _s()
    signal $s()
    signal _1s()
    signal __()
    on_ZChanged: console.log("_z", _z)
    on__YChanged: console.log("__y", __y)
    on$DChanged: console.log("$d", $d)
    on_1QChanged: console.log("_1q", _1q)
    onPlainChanged: console.log("plain", plain)
    on_S: console.log("_s")
    on$S: console.log("$s")
    on_1S: console.log("_1s")
    on__: console.log("__")
    Connections {
        target: root
        function on_ZChanged() { console.log("connected _z") }
        function on$S() { console.log("connected $s") }
        function on_1QChanged() { console.log("connected _1q") }
    }
    Component.onCompleted: {
        console.log("completed");
        plain = 1;
        _1q = 1;
        $d = 1;
        __y = 1;
        _z = 1;
        _z = 2;
        _s();
        $s();
        _1s();
        __();
        Qt.quit();
    }
}
