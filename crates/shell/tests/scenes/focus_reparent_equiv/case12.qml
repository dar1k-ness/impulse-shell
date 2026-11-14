// copied delegates, each a scope with nothing focused before them
import QtQuick
import QtQuick.Controls
Item {
    id: root
    width: 300; height: 200
    
        Repeater { id: r; model: 0; FocusScope { property alias f: f_; TextInput { id: f_; focus: true } } }
        property Item d0: null
        property Item d3: null
    function st(n, i) { return n + "=" + (i.focus ? "f" : "-") + (i.activeFocus ? "a" : "-") }
    function step() { r.model = 4; d0 = r.itemAt(0).f; d3 = r.itemAt(3).f }
    function line() { return [st("d0", d0), st("d3", d3)].join(" ") }
}
