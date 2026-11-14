// delegate into a scope with no focus item
import QtQuick
import QtQuick.Controls
Item {
    id: root
    width: 300; height: 200
    
        Item { id: holder; Repeater { id: r; model: 0; Item { property alias f: f_; TextInput { id: f_; focus: true } } } }
        property Item field: null
    function st(n, i) { return n + "=" + (i.focus ? "f" : "-") + (i.activeFocus ? "a" : "-") }
    function step() { r.model = 1; field = r.itemAt(0).f }
    function line() { return [st("holder", holder), st("field", field)].join(" ") }
}
