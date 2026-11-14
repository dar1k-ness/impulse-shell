// four delegates: the third and fourth are copies of the recorded second
import QtQuick
import QtQuick.Controls
Item {
    id: root
    width: 300; height: 200
    
        Repeater { id: r; model: 0; Item { property alias f: f_; TextInput { id: f_; focus: true } } }
        property Item d0: null
        property Item d1: null
        property Item d2: null
        property Item d3: null
    function st(n, i) { return n + "=" + (i.focus ? "f" : "-") + (i.activeFocus ? "a" : "-") }
    function step() { r.model = 4; d0 = r.itemAt(0).f; d1 = r.itemAt(1).f; d2 = r.itemAt(2).f; d3 = r.itemAt(3).f }
    function line() { return [st("d0", d0), st("d1", d1), st("d2", d2), st("d3", d3)].join(" ") }
}
