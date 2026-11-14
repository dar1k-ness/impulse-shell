// copied delegates with delegates of their own
import QtQuick
import QtQuick.Controls
Item {
    id: root
    width: 300; height: 200
    
        Repeater { id: r; model: 0
            Item { property alias ir: ir_; Repeater { id: ir_; model: 2; TextInput { focus: true } } }
        }
        property Item d00: null
        property Item d01: null
        property Item d30: null
        property Item d31: null
    function st(n, i) { return n + "=" + (i.focus ? "f" : "-") + (i.activeFocus ? "a" : "-") }
    function step() { r.model = 4; d00 = r.itemAt(0).ir.itemAt(0); d01 = r.itemAt(0).ir.itemAt(1); d30 = r.itemAt(3).ir.itemAt(0); d31 = r.itemAt(3).ir.itemAt(1) }
    function line() { return [st("d00", d00), st("d01", d01), st("d30", d30), st("d31", d31)].join(" ") }
}
