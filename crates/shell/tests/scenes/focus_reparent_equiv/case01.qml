// static siblings: the first asks first
import QtQuick
import QtQuick.Controls
Item {
    id: root
    width: 300; height: 200
    
        TextInput { id: a; focus: true }
        TextInput { id: b; focus: true }
    function st(n, i) { return n + "=" + (i.focus ? "f" : "-") + (i.activeFocus ? "a" : "-") }
    function step() {  }
    function line() { return [st("a", a), st("b", b)].join(" ") }
}
