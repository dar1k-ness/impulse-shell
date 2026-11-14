// two delegates at once
import QtQuick
import QtQuick.Controls
Item {
    id: root
    width: 300; height: 200
    
        Repeater { id: r; model: 0; Item { property alias f: f_; TextInput { id: f_; focus: true } } }
        property Item first: null
        property Item second: null
    function st(n, i) { return n + "=" + (i.focus ? "f" : "-") + (i.activeFocus ? "a" : "-") }
    function step() { r.model = 2; first = r.itemAt(0).f; second = r.itemAt(1).f }
    function line() { return [st("first", first), st("second", second)].join(" ") }
}
