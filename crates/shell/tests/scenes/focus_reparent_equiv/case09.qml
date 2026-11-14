// createObject with a parent
import QtQuick
import QtQuick.Controls
Item {
    id: root
    width: 300; height: 200
    
        TextInput { id: a; focus: true }
        Item { id: holder }
        Component { id: comp; Item { property alias f: f_; TextInput { id: f_; focus: true } } }
        property Item field: null
    function st(n, i) { return n + "=" + (i.focus ? "f" : "-") + (i.activeFocus ? "a" : "-") }
    function step() { field = comp.createObject(holder).f }
    function line() { return [st("a", a), st("field", field)].join(" ") }
}
