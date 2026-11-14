// overlay: content item has focus, a delegate's field asks for it
import QtQuick
import QtQuick.Controls
Item {
    id: root
    width: 300; height: 200
    
        Item { id: content; focus: true
            Item { Repeater { id: r; model: 0; Item { property alias f: f_; Item { TextField { id: f_; focus: true } } } } }
        }
        property Item field: null
    function st(n, i) { return n + "=" + (i.focus ? "f" : "-") + (i.activeFocus ? "a" : "-") }
    function step() { r.model = 1; field = r.itemAt(0).f }
    function line() { return [st("content", content), st("field", field)].join(" ") }
}
