// static: a focused item's descendants ask too
import QtQuick
import QtQuick.Controls
Item {
    id: root
    width: 300; height: 200
    
        Item { id: a; focus: true
            Item { TextInput { id: b; focus: true } }
            TextInput { id: c; focus: true }
        }
    function st(n, i) { return n + "=" + (i.focus ? "f" : "-") + (i.activeFocus ? "a" : "-") }
    function step() {  }
    function line() { return [st("a", a), st("b", b), st("c", c)].join(" ") }
}
