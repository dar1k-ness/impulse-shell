// Loader is a scope of its own
import QtQuick
import QtQuick.Controls
Item {
    id: root
    width: 300; height: 200
    
        TextInput { id: a; focus: true }
        Loader { id: l; active: false; sourceComponent: Item { property alias f: f_; TextInput { id: f_; focus: true } } }
        property Item field: null
    function st(n, i) { return n + "=" + (i.focus ? "f" : "-") + (i.activeFocus ? "a" : "-") }
    function step() { l.active = true; field = l.item.f }
    function line() { return [st("a", a), st("l", l), st("field", field)].join(" ") }
}
