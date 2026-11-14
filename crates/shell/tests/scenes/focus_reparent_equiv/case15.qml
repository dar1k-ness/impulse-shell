// a focused item moved into a scope that has a focus item, and into one that has none
import QtQuick
import QtQuick.Controls
Item {
    id: root
    width: 300; height: 200
    
        FocusScope { id: from; focus: true
            TextInput { id: a; focus: true }
        }
        FocusScope { id: full; TextInput { id: b; focus: true } }
        FocusScope { id: empty; Item { id: slot } }
        FocusScope { id: from2; TextInput { id: c; focus: true } }
    function st(n, i) { return n + "=" + (i.focus ? "f" : "-") + (i.activeFocus ? "a" : "-") }
    function step() { a.parent = full; c.parent = slot }
    function line() { return [st("from", from), st("a", a), st("full", full), st("b", b), st("empty", empty), st("c", c)].join(" ") }
}
