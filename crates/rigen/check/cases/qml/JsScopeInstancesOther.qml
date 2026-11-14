import QtQuick
import "js_scope_instances.js" as Plain
Item {
    property string tag: "other"
    width: 77
    function bump() { return Plain.bump() }
    function get() { return Plain.get() }
    function who() { return Plain.who() }
}
