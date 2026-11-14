// delegate that is a FocusScope keeps its own focus item
import QtQuick
import QtQuick.Controls
Item {
    id: root
    width: 300; height: 200
    
        TextInput { id: a; focus: true }
        Repeater { id: r; model: 0; FocusScope { property alias f: f_; TextInput { id: f_; focus: true } } }
        property Item deleg: null
        property Item field: null
    function st(n, i) { return n + "=" + (i.focus ? "f" : "-") + (i.activeFocus ? "a" : "-") }
    function step() { r.model = 1; deleg = r.itemAt(0); field = deleg.f }
    function line() { return [st("a", a), st("deleg", deleg), st("field", field)].join(" ") }
}
