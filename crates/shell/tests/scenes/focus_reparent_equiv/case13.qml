// copies of an object that lost its focus, made in a scope with no focus item
import QtQuick
import QtQuick.Controls
Item {
    id: root
    width: 300; height: 200
    
        FocusScope { id: s1 }
        FocusScope { id: s2 }
        Component { id: comp; Item { property alias f: f_; TextInput { id: f_; focus: true } } }
        property Item d0: null
        property Item d1: null
        property Item d2: null
        property Item d3: null
    function st(n, i) { return n + "=" + (i.focus ? "f" : "-") + (i.activeFocus ? "a" : "-") }
    function step() { d0 = comp.createObject(s1).f; d1 = comp.createObject(s1).f; d2 = comp.createObject(s2).f; d3 = comp.createObject(s2).f }
    function line() { return [st("d0", d0), st("d1", d1), st("d2", d2), st("d3", d3)].join(" ") }
}
