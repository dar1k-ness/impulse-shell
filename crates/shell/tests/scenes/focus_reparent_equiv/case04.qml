// delegate into a scope whose focus item is a sibling
import QtQuick
import QtQuick.Controls
Item {
    id: root
    width: 300; height: 200
    
        TextInput { id: a; focus: true }
        Repeater { id: r; model: 0; Item { property alias f: f_; TextInput { id: f_; focus: true } } }
        property Item field: null
    function st(n, i) { return n + "=" + (i.focus ? "f" : "-") + (i.activeFocus ? "a" : "-") }
    function step() { r.model = 1; field = r.itemAt(0).f }
    function line() { return [st("a", a), st("field", field)].join(" ") }
}
